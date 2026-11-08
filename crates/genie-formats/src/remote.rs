use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;

use crate::Error;
use crate::http::{self, Url};

const BLOCK: u64 = 64 << 10;
const BUDGET: usize = 64 << 20;
const RUN: u64 = 16;
const PARALLEL: usize = 8;

#[derive(Debug)]
pub struct Remote {
    url: Url,
    key: String,
    size: u64,
    position: u64,
    next: u64,
    run: u64,
}

impl Remote {
    pub fn open(url: &Url) -> Result<Self, Error> {
        let key = url.to_string();
        let known = cache().sizes.get(&key).copied();
        let size = if let Some(size) = known {
            size
        } else {
            let fetched = http::fetch(url, 0, BLOCK)?;
            let mut cache = cache();
            cache.sizes.insert(key.clone(), fetched.size);
            cache.put(&key, 0, fetched.bytes.into());
            fetched.size
        };
        Ok(Self {
            url: url.clone(),
            key,
            size,
            position: 0,
            next: 1,
            run: 1,
        })
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn pieces<const N: usize>(&self, starts: &[u64]) -> Result<Vec<[u8; N]>, Error> {
        let mut found: Vec<Option<[u8; N]>> =
            starts.iter().map(|&start| self.cached(start)).collect();
        let missing: Vec<usize> = (0..starts.len())
            .filter(|&index| found[index].is_none())
            .collect();
        let share = missing.len().div_ceil(PARALLEL).max(1);
        thread::scope(|scope| {
            let workers: Vec<_> = missing
                .chunks(share)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|&index| Ok((index, self.piece(starts[index])?)))
                            .collect::<Result<Vec<_>, Error>>()
                    })
                })
                .collect();
            for worker in workers {
                let part = worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
                for (index, piece) in part {
                    found[index] = Some(piece);
                }
            }
            Ok::<_, Error>(())
        })?;
        Ok(found.into_iter().flatten().collect())
    }

    fn cached<const N: usize>(&self, start: u64) -> Option<[u8; N]> {
        let index = start / BLOCK;
        let offset = usize::try_from(start - index * BLOCK).ok()?;
        let block = cache().get(&self.key, index)?;
        block.get(offset..offset + N)?.try_into().ok()
    }

    fn piece<const N: usize>(&self, start: u64) -> Result<[u8; N], Error> {
        let fetched = http::fetch(&self.url, start, start + N as u64)?;
        fetched
            .bytes
            .try_into()
            .map_err(|_| Error::Remote(format!("The server sent too few bytes for {}.", self.url)))
    }

    fn block(&mut self, index: u64) -> io::Result<Arc<[u8]>> {
        if let Some(block) = cache().get(&self.key, index) {
            return Ok(block);
        }
        self.run = if index == self.next {
            (self.run * 2).min(RUN)
        } else {
            1
        };
        let start = index * BLOCK;
        let end = ((index + self.run) * BLOCK).min(self.size);
        let fetched = http::fetch(&self.url, start, end).map_err(io::Error::other)?;
        self.next = index + self.run;
        let mut cache = cache();
        let mut first = None;
        for (offset, part) in fetched.bytes.chunks(BLOCK as usize).enumerate() {
            let block: Arc<[u8]> = part.into();
            first.get_or_insert_with(|| Arc::clone(&block));
            cache.put(&self.key, index + offset as u64, block);
        }
        first.ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))
    }
}

impl Read for Remote {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.position >= self.size || buffer.is_empty() {
            return Ok(0);
        }
        let index = self.position / BLOCK;
        let block = self.block(index)?;
        let offset = (self.position - index * BLOCK) as usize;
        let count = buffer.len().min(block.len().saturating_sub(offset));
        if count == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        buffer[..count].copy_from_slice(&block[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for Remote {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let position = match to {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
        };
        self.position = position.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the position is before the start",
            )
        })?;
        Ok(self.position)
    }
}

type Key = (String, u64);
type Slot = (Arc<[u8]>, u64);

struct Cache {
    budget: usize,
    used: usize,
    clock: u64,
    blocks: HashMap<Key, Slot>,
    sizes: HashMap<String, u64>,
}

impl Cache {
    fn new(budget: usize) -> Self {
        Self {
            budget,
            used: 0,
            clock: 0,
            blocks: HashMap::new(),
            sizes: HashMap::new(),
        }
    }

    fn get(&mut self, url: &str, index: u64) -> Option<Arc<[u8]>> {
        self.clock += 1;
        let clock = self.clock;
        let (block, used) = self.blocks.get_mut(&(url.to_owned(), index))?;
        *used = clock;
        Some(Arc::clone(block))
    }

    fn put(&mut self, url: &str, index: u64, block: Arc<[u8]>) {
        self.clock += 1;
        self.used += block.len();
        if let Some((old, _)) = self
            .blocks
            .insert((url.to_owned(), index), (block, self.clock))
        {
            self.used -= old.len();
        }
        while self.used > self.budget {
            let Some(oldest) = self
                .blocks
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((block, _)) = self.blocks.remove(&oldest) {
                self.used -= block.len();
            }
        }
    }
}

fn cache() -> std::sync::MutexGuard<'static, Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE
        .get_or_init(|| Mutex::new(Cache::new(BUDGET)))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use super::{Cache, Remote};
    use crate::http::Url;
    use crate::server::{Server, serve};

    fn data(size: usize) -> Vec<u8> {
        (0..size).map(|index| (index * 7 % 251) as u8).collect()
    }

    fn served(size: usize) -> (Server, Url) {
        let server = serve(vec![("/a.bin".to_owned(), data(size))]);
        let url = Url::parse(&server.url("/a.bin")).unwrap();
        (server, url)
    }

    #[test]
    fn remote_files_read_and_seek_like_local_ones() {
        let (_server, url) = served(300_000);
        let mut remote = Remote::open(&url).unwrap();
        assert_eq!(remote.size(), 300_000);
        let mut all = Vec::new();
        remote.read_to_end(&mut all).unwrap();
        assert_eq!(all, data(300_000));
        remote.seek(SeekFrom::Start(200_000)).unwrap();
        let mut part = [0; 10];
        remote.read_exact(&mut part).unwrap();
        assert_eq!(part, data(300_000)[200_000..200_010]);
        assert_eq!(remote.seek(SeekFrom::End(-5)).unwrap(), 299_995);
        assert_eq!(remote.seek(SeekFrom::Current(-95)).unwrap(), 299_900);
        assert!(remote.seek(SeekFrom::Current(-300_000)).is_err());
    }

    #[test]
    fn blocks_come_from_the_cache_the_second_time() {
        let (server, url) = served(200_000);
        let mut remote = Remote::open(&url).unwrap();
        let mut part = [0; 100];
        remote.seek(SeekFrom::Start(150_000)).unwrap();
        remote.read_exact(&mut part).unwrap();
        let asked = server.requests();
        let mut again = Remote::open(&url).unwrap();
        again.seek(SeekFrom::Start(150_000)).unwrap();
        again.read_exact(&mut part).unwrap();
        assert_eq!(server.requests(), asked);
    }

    #[test]
    fn long_reads_ask_for_longer_runs() {
        let (server, url) = served(2 << 20);
        let mut all = Vec::new();
        Remote::open(&url).unwrap().read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 2 << 20);
        assert!(server.requests() <= 8, "{}", server.requests());
    }

    #[test]
    fn pieces_come_from_the_cache_or_from_parallel_requests() {
        let server = serve(vec![("/a.bin".to_owned(), data(300_000))]);
        let url = Url::parse(&server.url("/slow/a.bin")).unwrap();
        let remote = Remote::open(&url).unwrap();
        let asked = server.requests();
        let starts = [10, 100_000, 150_000, 200_000, 250_000, 299_996];
        let pieces: Vec<[u8; 4]> = remote.pieces(&starts).unwrap();
        let expected: Vec<[u8; 4]> = starts
            .iter()
            .map(|&start| {
                let start = usize::try_from(start).unwrap();
                data(300_000)[start..start + 4].try_into().unwrap()
            })
            .collect();
        assert_eq!(pieces, expected);
        assert_eq!(server.requests(), asked + 5);
        assert!(server.connections() > 1, "{}", server.connections());
    }

    #[test]
    fn pieces_past_the_end_fail() {
        let (_server, url) = served(1_000);
        let remote = Remote::open(&url).unwrap();
        let last: [u8; 4] = data(1_000)[996..].try_into().unwrap();
        assert_eq!(remote.pieces::<4>(&[996]).unwrap(), [last]);
        assert!(remote.pieces::<4>(&[998]).is_err());
    }

    #[test]
    fn missing_files_fail_to_open() {
        let (server, _) = served(10);
        let url = Url::parse(&server.url("/none.bin")).unwrap();
        let error = Remote::open(&url).err().unwrap().to_string();
        assert!(error.starts_with("The server answered 404"), "{error}");
    }

    #[test]
    fn the_cache_drops_the_oldest_blocks_past_its_budget() {
        let mut cache = Cache::new(25);
        let url = "http://a/b".to_owned();
        for block in 0..3 {
            cache.put(&url, block, vec![0; 10].into());
        }
        assert!(cache.get(&url, 0).is_none());
        assert!(cache.get(&url, 1).is_some() && cache.get(&url, 2).is_some());
        cache.get(&url, 1);
        cache.put(&url, 3, vec![0; 10].into());
        assert!(cache.get(&url, 1).is_some());
        assert!(cache.get(&url, 2).is_none());
    }
}
