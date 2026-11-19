#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

const MEETING: Duration = Duration::from_secs(1);
/// The time that the server takes for a request of the `/hold` route.
const HOLD: Duration = Duration::from_millis(100);

/// What the server counts: the connections, the requests, the requests that meet on the
/// `/slow` route, the requests in work and the most of them at one time, and the requests of
/// each path.
#[derive(Default)]
struct Counts {
    connections: AtomicUsize,
    requests: AtomicUsize,
    inflight: AtomicUsize,
    working: AtomicUsize,
    peak: AtomicUsize,
    paths: Mutex<HashMap<String, usize>>,
}

pub struct Server {
    pub port: u16,
    counts: Arc<Counts>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn connections(&self) -> usize {
        self.counts.connections.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.counts.requests.load(Ordering::SeqCst)
    }

    /// The most requests that the server had in work at one time.
    pub fn peak(&self) -> usize {
        self.counts.peak.load(Ordering::SeqCst)
    }
}

pub fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let files: Arc<HashMap<String, Vec<u8>>> = Arc::new(files.into_iter().collect());
    let counts = Arc::new(Counts::default());
    let shared = Arc::clone(&counts);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                break;
            };
            shared.connections.fetch_add(1, Ordering::SeqCst);
            let (files, counts) = (Arc::clone(&files), Arc::clone(&shared));
            thread::spawn(move || answer(stream, &files, &counts));
        }
    });
    Server { port, counts }
}

/// A request in work: it counts the requests in work and their peak while it lives.
struct Work<'a>(&'a Counts);

impl<'a> Work<'a> {
    fn new(counts: &'a Counts) -> Self {
        let now = counts.working.fetch_add(1, Ordering::SeqCst) + 1;
        counts.peak.fetch_max(now, Ordering::SeqCst);
        Self(counts)
    }
}

impl Drop for Work<'_> {
    fn drop(&mut self) {
        self.0.working.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The status and the Retry-After seconds of a busy server, and the rest of the path. The
/// `/busy` route is busy for the first two requests of each path, the `/jammed` route always,
/// and the `/later` route asks for an hour.
type Busy<'a> = (Option<(&'static str, &'static str)>, &'a str);

fn busy<'a>(path: &'a str, counts: &Counts) -> Busy<'a> {
    let asked = {
        let mut paths = counts.paths.lock().unwrap_or_else(PoisonError::into_inner);
        let asked = paths.entry(path.to_owned()).or_default();
        *asked += 1;
        *asked
    };
    let (twice, path) = route(path, "/busy");
    let (always, path) = route(path, "/jammed");
    let (later, path) = route(path, "/later");
    let answer = if later {
        Some(("429 Too Many Requests", "3600"))
    } else if always || (twice && asked <= 2) {
        Some(("503 Service Unavailable", "0"))
    } else {
        None
    };
    (answer, path)
}

struct Meeting<'a>(&'a AtomicUsize);

impl<'a> Meeting<'a> {
    fn new(inflight: &'a AtomicUsize) -> Self {
        inflight.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + MEETING;
        while inflight.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        Self(inflight)
    }
}

impl Drop for Meeting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn answer(stream: TcpStream, files: &HashMap<String, Vec<u8>>, counts: &Counts) {
    let Ok(copy) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(copy);
    let mut writer = stream;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
        let mut range = None;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(value) = header.strip_prefix("Range: bytes=")
                && let Some((start, end)) = value.split_once('-')
            {
                range = Some((
                    start.parse::<usize>().unwrap(),
                    end.parse::<usize>().unwrap(),
                ));
            }
        }
        counts.requests.fetch_add(1, Ordering::SeqCst);
        let _work = Work::new(counts);
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        let (busy, path) = busy(path, counts);
        if let Some((status, wait)) = busy {
            let head =
                format!("HTTP/1.1 {status}\r\nRetry-After: {wait}\r\nContent-Length: 0\r\n\r\n");
            if writer.write_all(head.as_bytes()).is_err() {
                return;
            }
            continue;
        }
        if let Some(rest) = path.strip_prefix("/redirect") {
            let location = if query.is_empty() {
                rest.to_owned()
            } else {
                format!("{rest}?{query}")
            };
            let head =
                format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n");
            if writer.write_all(head.as_bytes()).is_err() {
                return;
            }
            continue;
        }
        // As the GitHub API: parts say `bytes 0-4194303/*`, the range asked for and no size.
        let (sizeless, path) = route(path, "/unsized");
        let (chunked, path) = route(path, "/chunked");
        let (whole, path) = route(path, "/whole");
        let (close, path) = route(path, "/close");
        // No length and no chunks: the data ends where the connection ends.
        let (eof, path) = route(path, "/eof");
        let (slow, path) = route(path, "/slow");
        let _meeting = slow.then(|| Meeting::new(&counts.inflight));
        let (hold, path) = route(path, "/hold");
        if hold {
            thread::sleep(HOLD);
        }
        let Some(data) = files.get(path) else {
            let head = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            if writer.write_all(head.as_bytes()).is_err() {
                return;
            }
            continue;
        };
        let close = close || eof;
        let framing = (chunked, close, eof);
        let message = message(data, range.filter(|_| !whole), framing, sizeless);
        if writer.write_all(&message).is_err() || close {
            return;
        }
    }
}

fn message(
    data: &[u8],
    range: Option<(usize, usize)>,
    (chunked, close, eof): (bool, bool, bool),
    sizeless: bool,
) -> Vec<u8> {
    let (status, extra, body) = match range {
        Some((start, _)) if start >= data.len() => (
            "416 Range Not Satisfiable",
            format!("Content-Range: bytes */{}\r\n", data.len()),
            &data[0..0],
        ),
        Some((start, asked)) => {
            let end = (asked + 1).min(data.len());
            let part = if sizeless {
                format!("{start}-{asked}/*")
            } else {
                format!("{start}-{}/{}", end - 1, data.len())
            };
            (
                "206 Partial Content",
                format!("Content-Range: bytes {part}\r\n"),
                &data[start..end],
            )
        }
        None => ("200 OK", String::new(), data),
    };
    let mut head = format!("HTTP/1.1 {status}\r\n{extra}");
    if close {
        head.push_str("Connection: close\r\n");
    }
    let mut message = Vec::new();
    if chunked {
        head.push_str("Transfer-Encoding: chunked\r\n\r\n");
        message.extend_from_slice(head.as_bytes());
        for part in body.chunks(body.len().div_ceil(3).max(1)) {
            message.extend_from_slice(format!("{:x};note=yes\r\n", part.len()).as_bytes());
            message.extend_from_slice(part);
            message.extend_from_slice(b"\r\n");
        }
        message.extend_from_slice(b"0\r\n\r\n");
    } else if eof {
        head.push_str("\r\n");
        message.extend_from_slice(head.as_bytes());
        message.extend_from_slice(body);
    } else {
        let length = format!("Content-Length: {}\r\n\r\n", body.len());
        head.push_str(&length);
        message.extend_from_slice(head.as_bytes());
        message.extend_from_slice(body);
    }
    message
}

fn route<'a>(path: &'a str, prefix: &str) -> (bool, &'a str) {
    match path.strip_prefix(prefix) {
        Some(rest) => (true, rest),
        None => (false, path),
    }
}
