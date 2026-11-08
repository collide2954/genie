use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use rustls_platform_verifier::BuilderVerifierExt;

use crate::Error;

const CONNECT: Duration = Duration::from_secs(10);
const WAIT: Duration = Duration::from_secs(30);
const REDIRECTS: usize = 5;
const WHOLE: u64 = 4 << 20;
const IDLE: usize = 4;
const AGENT: &str = concat!("genie/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url {
    secure: bool,
    host: String,
    port: u16,
    target: String,
}

impl Url {
    pub fn is_url(text: &str) -> bool {
        scheme(text.trim()).is_some()
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        let (secure, rest) = scheme(text).ok_or_else(|| {
            remote(format!(
                "The address {text} does not start with https:// or http://."
            ))
        })?;
        let (authority, target) = match rest.find(['/', '?']) {
            Some(at) if rest[at..].starts_with('?') => (&rest[..at], format!("/{}", &rest[at..])),
            Some(at) => (&rest[..at], rest[at..].to_owned()),
            None => (rest, "/".to_owned()),
        };
        if authority.contains('@') {
            return Err(remote(format!(
                "genie does not send user names or passwords in addresses: {text}."
            )));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.contains(']') => {
                let port = port
                    .parse()
                    .map_err(|_| remote(format!("The port in {text} is not a number.")))?;
                (host, Some(port))
            }
            _ => (authority, None),
        };
        if host.is_empty() {
            return Err(remote(format!("The address {text} has no server name.")));
        }
        Ok(Self {
            secure,
            host: host.to_ascii_lowercase(),
            port: port.unwrap_or(if secure { 443 } else { 80 }),
            target,
        })
    }

    pub fn name(&self) -> String {
        let path = self.target.split('?').next().unwrap_or_default();
        match path.rsplit('/').next() {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => self.host.clone(),
        }
    }

    #[must_use]
    pub fn with_suffix(&self, suffix: &str) -> Self {
        let target = match self.target.split_once('?') {
            Some((path, query)) => format!("{path}{suffix}?{query}"),
            None => format!("{}{suffix}", self.target),
        };
        Self {
            target,
            ..self.clone()
        }
    }

    fn resolve(&self, location: &str) -> Result<Self, Error> {
        if Self::is_url(location) {
            return Self::parse(location);
        }
        if location.starts_with("//") {
            let scheme = if self.secure { "https:" } else { "http:" };
            return Self::parse(&format!("{scheme}{location}"));
        }
        let target = if location.starts_with('/') {
            location.to_owned()
        } else {
            let path = self.target.split('?').next().unwrap_or("/");
            format!("{}{location}", &path[..=path.rfind('/').unwrap_or(0)])
        };
        Ok(Self {
            target,
            ..self.clone()
        })
    }

    fn authority(&self) -> String {
        if self.port == if self.secure { 443 } else { 80 } {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    fn origin(&self) -> Origin {
        (self.secure, self.host.clone(), self.port)
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.secure { "https" } else { "http" };
        write!(f, "{scheme}://{}{}", self.authority(), self.target)
    }
}

fn scheme(text: &str) -> Option<(bool, &str)> {
    for (prefix, secure) in [("https://", true), ("http://", false)] {
        if text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return Some((secure, &text[prefix.len()..]));
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub size: u64,
}

pub fn fetch(url: &Url, start: u64, end: u64) -> Result<Fetched, Error> {
    let last = end.max(start + 1) - 1;
    let mut current = url.clone();
    for _ in 0..=REDIRECTS {
        let answer = exchange(&current, start, last)?;
        match answer.status {
            206 => {
                let size = answer.size().ok_or_else(|| bad_answer(url))?;
                let mut bytes = answer.body;
                bytes.truncate(usize::try_from(end - start).unwrap_or(usize::MAX));
                return Ok(Fetched { bytes, size });
            }
            200 => {
                let size = answer.body.len() as u64;
                let (low, high) = (start.min(size) as usize, end.min(size) as usize);
                return Ok(Fetched {
                    bytes: answer.body[low..high].to_vec(),
                    size,
                });
            }
            416 => {
                let size = answer.size().ok_or_else(|| bad_answer(url))?;
                return Ok(Fetched {
                    bytes: Vec::new(),
                    size,
                });
            }
            301 | 302 | 303 | 307 | 308 => {
                let location = answer.header("location").ok_or_else(|| bad_answer(url))?;
                current = current.resolve(location)?;
            }
            status => {
                return Err(remote(format!(
                    "The server answered {status} {} for {url}.",
                    answer.reason
                )));
            }
        }
    }
    Err(remote(format!("The address {url} has too many redirects.")))
}

type Origin = (bool, String, u16);

struct Answer {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn size(&self) -> Option<u64> {
        self.header("content-range")?
            .rsplit('/')
            .next()?
            .trim()
            .parse()
            .ok()
    }
}

enum Stream {
    Plain(TcpStream),
    Secure(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buffer),
            Self::Secure(stream) => stream.read(buffer),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buffer),
            Self::Secure(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Secure(stream) => stream.flush(),
        }
    }
}

struct Connection {
    reader: BufReader<Stream>,
}

impl Connection {
    fn open(url: &Url) -> Result<Self, Error> {
        let host = url.host.trim_start_matches('[').trim_end_matches(']');
        let addresses = (host, url.port)
            .to_socket_addrs()
            .map_err(|error| remote(format!("genie cannot find the server {host}. {error}")))?;
        let mut failure = None;
        let tcp = addresses
            .into_iter()
            .find_map(
                |address| match TcpStream::connect_timeout(&address, CONNECT) {
                    Ok(stream) => Some(stream),
                    Err(error) => {
                        failure = Some(error);
                        None
                    }
                },
            )
            .ok_or_else(|| {
                let reason = failure.map(|error| format!(" {error}")).unwrap_or_default();
                remote(format!("genie cannot connect to {host}.{reason}"))
            })?;
        tcp.set_read_timeout(Some(WAIT))?;
        tcp.set_write_timeout(Some(WAIT))?;
        tcp.set_nodelay(true)?;
        let stream = if url.secure {
            let name = ServerName::try_from(host.to_owned())
                .map_err(|_| remote(format!("The server name {host} is not valid.")))?;
            let connection =
                ClientConnection::new(tls()?, name).map_err(|error| insecure(host, &error))?;
            Stream::Secure(Box::new(StreamOwned::new(connection, tcp)))
        } else {
            Stream::Plain(tcp)
        };
        Ok(Self {
            reader: BufReader::new(stream),
        })
    }

    fn send(&mut self, url: &Url, start: u64, last: u64) -> Result<(Answer, bool), Error> {
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {AGENT}\r\nAccept-Encoding: identity\r\nRange: bytes={start}-{last}\r\n\r\n",
            url.target,
            url.authority()
        );
        let stream = self.reader.get_mut();
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        self.receive(url)
    }

    fn receive(&mut self, url: &Url) -> Result<(Answer, bool), Error> {
        let status_line = line(&mut self.reader)?;
        let mut parts = status_line.splitn(3, ' ');
        let version = parts.next().unwrap_or_default().to_owned();
        let status = parts
            .next()
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| bad_answer(url))?;
        let reason = parts.next().unwrap_or_default().to_owned();
        let mut headers = Vec::new();
        loop {
            let header = line(&mut self.reader)?;
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
            }
        }
        let mut answer = Answer {
            status,
            reason,
            headers,
            body: Vec::new(),
        };
        let chunked = answer
            .header("transfer-encoding")
            .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
        let length = answer
            .header("content-length")
            .and_then(|value| value.parse::<u64>().ok());
        if status == 200 && length.is_none_or(|length| length > WHOLE) {
            return Err(remote(format!(
                "The server of {url} does not send parts of files."
            )));
        }
        answer.body = if chunked {
            chunks(&mut self.reader)?
        } else if let Some(length) = length {
            let mut body = vec![0; usize::try_from(length).map_err(|_| bad_answer(url))?];
            self.reader.read_exact(&mut body)?;
            body
        } else {
            let mut body = Vec::new();
            self.reader.read_to_end(&mut body)?;
            body
        };
        let closes = answer
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("close"));
        let keep = version == "HTTP/1.1" && !closes && (chunked || length.is_some());
        Ok((answer, keep))
    }
}

fn exchange(url: &Url, start: u64, last: u64) -> Result<Answer, Error> {
    let origin = url.origin();
    if let Some(mut connection) = take(&origin)
        && let Ok((answer, keep)) = connection.send(url, start, last)
    {
        if keep {
            give(origin, connection);
        }
        return Ok(answer);
    }
    let mut connection = Connection::open(url)?;
    let (answer, keep) = connection
        .send(url, start, last)
        .map_err(|error| failed(url, error))?;
    if keep {
        give(origin, connection);
    }
    Ok(answer)
}

fn line(reader: &mut impl BufRead) -> Result<String, Error> {
    let mut bytes = Vec::new();
    if reader.read_until(b'\n', &mut bytes)? == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
    }
    Ok(String::from_utf8_lossy(&bytes).trim_end().to_owned())
}

fn chunks(reader: &mut impl BufRead) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    loop {
        let size_line = line(reader)?;
        let size = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size, 16)
            .map_err(|_| remote("A server sent a chunk size that is not valid.".to_owned()))?;
        if size == 0 {
            while !line(reader)?.is_empty() {}
            return Ok(body);
        }
        let at = body.len();
        body.resize(at + size, 0);
        reader.read_exact(&mut body[at..])?;
        line(reader)?;
    }
}

fn pool() -> &'static Mutex<HashMap<Origin, Vec<Connection>>> {
    static POOL: OnceLock<Mutex<HashMap<Origin, Vec<Connection>>>> = OnceLock::new();
    POOL.get_or_init(Mutex::default)
}

fn take(origin: &Origin) -> Option<Connection> {
    pool()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_mut(origin)?
        .pop()
}

fn give(origin: Origin, connection: Connection) {
    let mut pool = pool().lock().unwrap_or_else(PoisonError::into_inner);
    let idle = pool.entry(origin).or_default();
    if idle.len() < IDLE {
        idle.push(connection);
    }
}

fn tls() -> Result<Arc<ClientConfig>, Error> {
    static CONFIG: OnceLock<Result<Arc<ClientConfig>, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .and_then(BuilderVerifierExt::with_platform_verifier)
                .map(|builder| Arc::new(builder.with_no_client_auth()))
                .map_err(|error| format!("genie cannot set up secure connections. {error}"))
        })
        .clone()
        .map_err(remote)
}

fn failed(url: &Url, error: Error) -> Error {
    match error {
        Error::Io(error) => match error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        {
            Some(tls) => insecure(&url.host, tls),
            None => remote(format!("genie cannot read {url}. {error}")),
        },
        error => error,
    }
}

fn insecure(host: &str, error: &rustls::Error) -> Error {
    remote(format!("The connection to {host} is not secure. {error}"))
}

fn bad_answer(url: &Url) -> Error {
    remote(format!(
        "The server of {url} sent an answer that is not valid."
    ))
}

fn remote(message: String) -> Error {
    Error::Remote(message)
}

#[cfg(test)]
mod tests {
    use super::{Url, fetch};
    use crate::server::serve;

    #[test]
    fn urls_keep_their_parts() {
        let url = Url::parse("https://Example.org/data/a.bam?token=x").unwrap();
        assert_eq!(url.to_string(), "https://example.org/data/a.bam?token=x");
        assert_eq!(url.name(), "a.bam");
        assert_eq!(
            url.with_suffix(".bai").to_string(),
            "https://example.org/data/a.bam.bai?token=x"
        );
        let plain = Url::parse("http://localhost:8080").unwrap();
        assert_eq!(plain.to_string(), "http://localhost:8080/");
        let default = Url::parse("https://example.org:443/x").unwrap();
        assert_eq!(default.to_string(), "https://example.org/x");
        assert!(Url::parse("ftp://example.org/x").is_err());
        assert!(Url::parse("https://user:secret@example.org/x").is_err());
        assert!(Url::parse("https:///x").is_err());
        assert!(Url::is_url("HTTPS://a/b"));
        assert!(!Url::is_url("/data/a.bam"));
    }

    fn data() -> Vec<u8> {
        (0..=255).cycle().take(5_000).collect()
    }

    #[test]
    fn ranges_come_back_with_the_file_size() {
        let server = serve(vec![("/a.bin".to_owned(), data())]);
        let url = Url::parse(&server.url("/a.bin")).unwrap();
        let fetched = fetch(&url, 100, 300).unwrap();
        assert_eq!(fetched.bytes, data()[100..300]);
        assert_eq!(fetched.size, 5_000);
        let tail = fetch(&url, 4_990, 6_000).unwrap();
        assert_eq!(tail.bytes, data()[4_990..]);
        let past = fetch(&url, 5_000, 6_000).unwrap();
        assert_eq!(past.bytes.len(), 0);
        assert_eq!(past.size, 5_000);
    }

    #[test]
    fn redirects_chunks_and_whole_answers_work() {
        let server = serve(vec![("/a.bin".to_owned(), data())]);
        for path in [
            "/redirect/a.bin",
            "/chunked/a.bin",
            "/whole/a.bin",
            "/redirect/chunked/a.bin",
        ] {
            let url = Url::parse(&server.url(path)).unwrap();
            let fetched = fetch(&url, 10, 20).unwrap();
            assert_eq!(fetched.bytes, data()[10..20], "{path}");
            assert_eq!(fetched.size, 5_000, "{path}");
        }
    }

    #[test]
    fn connections_are_kept_for_the_next_request() {
        let server = serve(vec![("/a.bin".to_owned(), data())]);
        let url = Url::parse(&server.url("/a.bin")).unwrap();
        for start in [0, 100, 200] {
            fetch(&url, start, start + 10).unwrap();
        }
        assert_eq!((server.connections(), server.requests()), (1, 3));
        let closing = Url::parse(&server.url("/close/a.bin")).unwrap();
        fetch(&closing, 0, 10).unwrap();
        assert_eq!(server.connections(), 1);
        fetch(&closing, 100, 110).unwrap();
        assert_eq!(server.connections(), 2);
    }

    #[test]
    fn missing_files_and_servers_say_so() {
        let server = serve(Vec::new());
        let url = Url::parse(&server.url("/none.bam")).unwrap();
        let error = fetch(&url, 0, 10).unwrap_err().to_string();
        assert_eq!(
            error,
            format!("The server answered 404 Not Found for {url}.")
        );
        let closed = Url::parse("http://127.0.0.1:9/a.bam").unwrap();
        let error = fetch(&closed, 0, 10).unwrap_err().to_string();
        assert!(
            error.starts_with("genie cannot connect to 127.0.0.1."),
            "{error}"
        );
    }

    #[test]
    #[ignore = "network"]
    fn https_works_with_the_os_certificates() {
        let url = "https://hgdownload.soe.ucsc.edu/goldenPath/hg38/bigZips/hg38.chrom.sizes";
        let fetched = fetch(&Url::parse(url).unwrap(), 0, 4).unwrap();
        assert_eq!(fetched.bytes, b"chr1");
        assert!(fetched.size > 1_000);
    }
}
