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
const PART: u64 = 4 << 20;
const IDLE: usize = 4;
const AGENT: &str = concat!("genie/", env!("CARGO_PKG_VERSION"));
const AMAZON: &str = "s3.amazonaws.com";
const GOOGLE: &str = "storage.googleapis.com";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Url {
    secure: bool,
    host: String,
    port: u16,
    target: String,
    cloud: Option<Cloud>,
}

/// A cloud store whose public buckets genie reads through HTTPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cloud {
    Amazon,
    Google,
}

impl Cloud {
    fn prefix(self) -> &'static str {
        match self {
            Self::Amazon => "s3://",
            Self::Google => "gs://",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Scheme {
    Web { secure: bool },
    Cloud(Cloud),
}

impl Url {
    pub fn is_url(text: &str) -> bool {
        scheme(text.trim()).is_some()
    }

    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        let (scheme, rest) = scheme(text).ok_or_else(|| {
            remote(format!(
                "The address {text} does not start with https://, http://, s3:// or gs://."
            ))
        })?;
        let secure = match scheme {
            Scheme::Web { secure } => secure,
            Scheme::Cloud(cloud) => return Self::in_bucket(cloud, rest, text),
        };
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
            cloud: None,
        })
    }

    /// The HTTPS address of an object in a public bucket. A bucket name that cannot be a part
    /// of a server name goes into the path.
    fn in_bucket(cloud: Cloud, rest: &str, text: &str) -> Result<Self, Error> {
        let (bucket, key) = rest.split_once('/').unwrap_or((rest, ""));
        let named = !bucket.is_empty()
            && bucket
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
        if !named {
            return Err(remote(format!("The bucket name in {text} is not valid.")));
        }
        let key = escape(key);
        let label = bucket
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        let (host, target) = match cloud {
            Cloud::Amazon if label => (format!("{bucket}.{AMAZON}"), format!("/{key}")),
            Cloud::Amazon => (AMAZON.to_owned(), format!("/{bucket}/{key}")),
            Cloud::Google => (GOOGLE.to_owned(), format!("/{bucket}/{key}")),
        };
        Ok(Self {
            secure: true,
            host,
            port: 443,
            target,
            cloud: Some(cloud),
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Where the file is: the bucket of a cloud address, or else the server.
    pub fn place(&self) -> String {
        match self.bucket() {
            Some((cloud, bucket, _)) => format!("{}{bucket}", cloud.prefix()),
            None => self.host.clone(),
        }
    }

    /// The cloud, the bucket and the key of an address in a cloud bucket.
    fn bucket(&self) -> Option<(Cloud, &str, &str)> {
        let cloud = self.cloud?;
        let path = self.target.strip_prefix('/').unwrap_or(&self.target);
        let named = self
            .host
            .strip_suffix(AMAZON)
            .and_then(|host| host.strip_suffix('.'));
        let (bucket, key) = named.map_or_else(
            || path.split_once('/').unwrap_or((path, "")),
            |bucket| (bucket, path),
        );
        Some((cloud, bucket, key))
    }

    pub fn name(&self) -> String {
        let path = self.target.split('?').next().unwrap_or_default();
        match path.rsplit('/').next() {
            Some(name) if !name.is_empty() => unescape(name),
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

    #[must_use]
    pub fn with_extension(&self, extension: &str) -> Self {
        let (path, query) = match self.target.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (self.target.as_str(), None),
        };
        let folder = path.rfind('/').map_or(0, |at| at + 1);
        let stem = path[folder..]
            .rfind('.')
            .map_or(path.len(), |dot| folder + dot);
        let path = format!("{}.{extension}", &path[..stem]);
        let target = match query {
            Some(query) => format!("{path}?{query}"),
            None => path,
        };
        Self {
            target,
            ..self.clone()
        }
    }

    /// The address of a location given relative to this one, as in a hub or a redirect.
    pub fn join(&self, location: &str) -> Result<Self, Error> {
        self.resolve(location)
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
            target: without_dots(&target),
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
        if let Some((cloud, bucket, key)) = self.bucket() {
            return write!(f, "{}{bucket}/{}", cloud.prefix(), decode(key, |_| true));
        }
        let scheme = if self.secure { "https" } else { "http" };
        write!(f, "{scheme}://{}{}", self.authority(), self.target)
    }
}

/// A part of a path with its %XX escapes decoded, except the escapes of characters that a file
/// name cannot hold. A part that does not decode to UTF-8 stays as it is.
fn unescape(part: &str) -> String {
    decode(part, |value| {
        value >= 0x80 || (value >= 0x20 && !b"<>:\"/\\|?*\x7f".contains(&value))
    })
}

/// The key of a cloud object as a path, with %XX escapes for all bytes except letters, digits,
/// `-_.~` and `/`.
fn escape(key: &str) -> String {
    let mut found = String::with_capacity(key.len());
    for byte in key.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~/".contains(&byte) {
            found.push(char::from(byte));
        } else {
            found.push('%');
            found.push(char::from(HEX[usize::from(byte >> 4)]));
            found.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    found
}

/// A part of a path with the %XX escapes decoded that `keep` accepts. A part that does not
/// decode to UTF-8 stays as it is.
fn decode(part: &str, keep: impl Fn(u8) -> bool) -> String {
    let bytes = part.as_bytes();
    let mut found = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let value = bytes
            .get(index + 1..index + 3)
            .filter(|hex| bytes[index] == b'%' && hex.iter().all(u8::is_ascii_hexdigit))
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok())
            .filter(|value| keep(*value));
        if let Some(value) = value {
            found.push(value);
            index += 3;
        } else {
            found.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(found).unwrap_or_else(|_| part.to_owned())
}

/// A path without its "." and ".." parts (RFC 3986), keeping any query.
fn without_dots(target: &str) -> String {
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };
    let mut parts: Vec<&str> = Vec::new();
    let pieces: Vec<&str> = path.split('/').collect();
    for (index, piece) in pieces.iter().enumerate() {
        let last = index + 1 == pieces.len();
        match *piece {
            "." => {
                if last {
                    parts.push("");
                }
            }
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                if last {
                    parts.push("");
                }
            }
            piece => parts.push(piece),
        }
    }
    let path = parts.join("/");
    let path = if path.starts_with('/') {
        path
    } else {
        format!("/{path}")
    };
    match query {
        Some(query) => format!("{path}?{query}"),
        None => path,
    }
}

fn scheme(text: &str) -> Option<(Scheme, &str)> {
    [
        ("https://", Scheme::Web { secure: true }),
        ("http://", Scheme::Web { secure: false }),
        ("s3://", Scheme::Cloud(Cloud::Amazon)),
        ("gs://", Scheme::Cloud(Cloud::Google)),
    ]
    .into_iter()
    .find_map(|(prefix, scheme)| {
        text.get(..prefix.len())
            .filter(|start| start.eq_ignore_ascii_case(prefix))
            .map(|_| (scheme, &text[prefix.len()..]))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub size: u64,
}

pub fn get(url: &Url) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    download(
        url,
        0,
        |part| {
            bytes.extend_from_slice(part);
            Ok(())
        },
        |_, _| {},
    )?;
    Ok(bytes)
}

pub fn download(
    url: &Url,
    from: u64,
    write: impl FnMut(&[u8]) -> io::Result<()>,
    progress: impl FnMut(u64, u64),
) -> Result<u64, Error> {
    download_in(url, from, PART, write, progress)
}

fn download_in(
    url: &Url,
    from: u64,
    part: u64,
    mut write: impl FnMut(&[u8]) -> io::Result<()>,
    mut progress: impl FnMut(u64, u64),
) -> Result<u64, Error> {
    let mut at = from;
    loop {
        let fetched = fetch(url, at, at + part)?;
        if at >= fetched.size {
            return Ok(fetched.size);
        }
        if fetched.bytes.is_empty() {
            return Err(remote(format!("The server sent no data for {url}.")));
        }
        write(&fetched.bytes)?;
        at += fetched.bytes.len() as u64;
        progress(at, fetched.size);
        if at >= fetched.size {
            return Ok(fetched.size);
        }
    }
}

pub fn fetch(url: &Url, start: u64, end: u64) -> Result<Fetched, Error> {
    let last = end.max(start + 1) - 1;
    let mut current = url.clone();
    for _ in 0..=REDIRECTS {
        let answer = exchange(&current, start, last)?;
        match answer.status {
            206 => {
                let wanted = usize::try_from(end - start).unwrap_or(usize::MAX);
                let (size, sizeless) = (answer.size(), answer.sizeless());
                let mut bytes = answer.body;
                let size = match size {
                    Some(size) => size,
                    // A server that does not know the size of the whole, as the GitHub API,
                    // ends Content-Range with `/*`. A part shorter than the range ends with the
                    // data. A full part cannot say where the file ends.
                    None if sizeless && bytes.len() < wanted => start + bytes.len() as u64,
                    None if sizeless => {
                        return Err(remote(format!(
                            "The server of {url} does not say the size of the file."
                        )));
                    }
                    None => return Err(bad_answer(url)),
                };
                bytes.truncate(wanted);
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
            status => return Err(refused(url, status, &answer.reason)),
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

    /// True when Content-Range ends with `/*`: the server does not know the size of the whole.
    fn sizeless(&self) -> bool {
        self.header("content-range")
            .is_some_and(|range| range.trim_end().ends_with("/*"))
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

fn refused(url: &Url, status: u16, reason: &str) -> Error {
    let public = if url.cloud.is_some() && matches!(status, 401 | 403) {
        " genie reads only public buckets."
    } else {
        ""
    };
    remote(format!(
        "The server answered {status} {reason} for {url}.{public}"
    ))
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
    use super::{Url, download_in, fetch, get, refused};
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
        assert_eq!(
            url.with_extension("bai").to_string(),
            "https://example.org/data/a.bai?token=x"
        );
        let bare = Url::parse("https://example.org/data/reads").unwrap();
        assert_eq!(
            bare.with_extension("bai").to_string(),
            "https://example.org/data/reads.bai"
        );
    }

    #[test]
    fn names_decode_the_escapes_that_file_names_can_hold() {
        let name = |text: &str| Url::parse(text).unwrap().name();
        assert_eq!(
            name("https://example.org/GSE1_primary%2Breplicate_loops.txt.gz?x=%2B"),
            "GSE1_primary+replicate_loops.txt.gz"
        );
        assert_eq!(name("https://example.org/my%20reads.bam"), "my reads.bam");
        assert_eq!(name("https://example.org/caf%C3%A9.bed"), "café.bed");
        assert_eq!(name("https://example.org/100%25.bed"), "100%.bed");
        assert_eq!(
            name("https://example.org/a%2Fb%3Ac%00.bed"),
            "a%2Fb%3Ac%00.bed"
        );
        assert_eq!(
            name("https://example.org/bad%zz%+1%A.bed"),
            "bad%zz%+1%A.bed"
        );
        assert_eq!(name("https://example.org/half%C3.bed"), "half%C3.bed");
    }

    #[test]
    fn cloud_addresses_read_public_buckets_over_https() {
        let amazon = Url::parse(" s3://1000genomes/phase3/data/HG00096/HG00096.bam ").unwrap();
        assert_eq!(
            (amazon.secure, amazon.host(), amazon.port),
            (true, "1000genomes.s3.amazonaws.com", 443)
        );
        assert_eq!(amazon.target, "/phase3/data/HG00096/HG00096.bam");
        assert_eq!(
            amazon.to_string(),
            "s3://1000genomes/phase3/data/HG00096/HG00096.bam"
        );
        assert_eq!(amazon.place(), "s3://1000genomes");
        assert_eq!(amazon.name(), "HG00096.bam");
        assert_eq!(
            amazon.with_suffix(".bai").to_string(),
            "s3://1000genomes/phase3/data/HG00096/HG00096.bam.bai"
        );
        let google = Url::parse("GS://gcp-public-data--gnomad/release/4.1/a.vcf.bgz").unwrap();
        assert_eq!(google.host(), "storage.googleapis.com");
        assert_eq!(
            google.target,
            "/gcp-public-data--gnomad/release/4.1/a.vcf.bgz"
        );
        assert_eq!(
            google.to_string(),
            "gs://gcp-public-data--gnomad/release/4.1/a.vcf.bgz"
        );
        assert_eq!(google.place(), "gs://gcp-public-data--gnomad");
        assert_eq!(
            google.with_extension("tbi").to_string(),
            "gs://gcp-public-data--gnomad/release/4.1/a.vcf.tbi"
        );
        let dotted = Url::parse("s3://My.Data/runs/x y+z%.bed").unwrap();
        assert_eq!(dotted.host(), "s3.amazonaws.com");
        assert_eq!(dotted.target, "/My.Data/runs/x%20y%2Bz%25.bed");
        assert_eq!(dotted.to_string(), "s3://My.Data/runs/x y+z%.bed");
        assert_eq!(dotted.place(), "s3://My.Data");
        assert_eq!(dotted.name(), "x y+z%.bed");
        assert_eq!(Url::parse(&dotted.to_string()).unwrap(), dotted);
        let hub = Url::parse("gs://bucket/hubs/mine/hub.txt").unwrap();
        assert_eq!(
            hub.join("../data/a.bb").unwrap().to_string(),
            "gs://bucket/hubs/data/a.bb"
        );
        assert!(Url::is_url("S3://a/b"));
        let web = Url::parse("https://example.org/a.bam").unwrap();
        assert_eq!(web.place(), "example.org");
        for wrong in [
            "s3://",
            "s3:///key",
            "gs://a b/c",
            "s3://user@bucket/c",
            "gs://bucket:80/c",
        ] {
            assert!(Url::parse(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn closed_buckets_say_that_genie_reads_only_public_buckets() {
        let closed = Url::parse("s3://closed/a.bam").unwrap();
        assert_eq!(
            refused(&closed, 403, "Forbidden").to_string(),
            "The server answered 403 Forbidden for s3://closed/a.bam. genie reads only public buckets."
        );
        let web = Url::parse("https://example.org/a.bam").unwrap();
        assert_eq!(
            refused(&web, 403, "Forbidden").to_string(),
            "The server answered 403 Forbidden for https://example.org/a.bam."
        );
    }

    #[test]
    fn relative_addresses_join_the_folder_of_the_file() {
        let hub = Url::parse("https://example.org/hubs/mine/hub.txt?x=1").unwrap();
        let join = |location: &str| hub.join(location).unwrap().to_string();
        assert_eq!(
            join("genomes.txt"),
            "https://example.org/hubs/mine/genomes.txt"
        );
        assert_eq!(
            join("hg38/trackDb.txt"),
            "https://example.org/hubs/mine/hg38/trackDb.txt"
        );
        assert_eq!(join("../data/a.bb"), "https://example.org/hubs/data/a.bb");
        assert_eq!(join("./a/../b.bw"), "https://example.org/hubs/mine/b.bw");
        assert_eq!(join("/top.bb"), "https://example.org/top.bb");
        assert_eq!(join("//cdn.org/c.bb"), "https://cdn.org/c.bb");
        assert_eq!(join("http://other.org/d.bb"), "http://other.org/d.bb");
        assert_eq!(join("../../../../e.bb"), "https://example.org/e.bb");
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
    fn downloads_come_in_parts_and_can_resume() {
        let server = serve(vec![
            ("/a.bin".to_owned(), data()),
            ("/empty.bin".to_owned(), Vec::new()),
        ]);
        let url = Url::parse(&server.url("/a.bin")).unwrap();
        let mut bytes = Vec::new();
        let mut steps = Vec::new();
        let size = download_in(
            &url,
            0,
            2_000,
            |part| {
                bytes.extend_from_slice(part);
                Ok(())
            },
            |done, size| steps.push((done, size)),
        )
        .unwrap();
        assert_eq!(size, 5_000);
        assert_eq!(bytes, data());
        assert_eq!(steps, [(2_000, 5_000), (4_000, 5_000), (5_000, 5_000)]);
        let mut tail = Vec::new();
        download_in(
            &url,
            4_500,
            2_000,
            |part| {
                tail.extend_from_slice(part);
                Ok(())
            },
            |_, _| {},
        )
        .unwrap();
        assert_eq!(tail, data()[4_500..]);
        assert_eq!(get(&url).unwrap(), data());
        let empty = Url::parse(&server.url("/empty.bin")).unwrap();
        assert_eq!(get(&empty).unwrap(), Vec::<u8>::new());
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

    /// The GitHub API answers a range with 206 and `Content-Range: bytes 0-4194303/*`, so it does
    /// not say the size of the whole answer. A part shorter than the range ends with the data.
    #[test]
    fn parts_of_an_unknown_size_end_where_their_data_ends() {
        let server = serve(vec![("/a.bin".to_owned(), data())]);
        let github = Url::parse(&server.url("/unsized/chunked/a.bin")).unwrap();
        assert_eq!(get(&github).unwrap(), data());
        let tail = fetch(&github, 4_000, 6_000).unwrap();
        assert_eq!((tail.bytes, tail.size), (data()[4_000..].to_vec(), 5_000));
        let full = Url::parse(&server.url("/unsized/a.bin")).unwrap();
        assert_eq!(
            fetch(&full, 0, 100).unwrap_err().to_string(),
            format!("The server of {full} does not say the size of the file.")
        );
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
