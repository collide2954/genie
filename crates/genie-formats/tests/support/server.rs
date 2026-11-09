#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MEETING: Duration = Duration::from_secs(1);

pub struct Server {
    pub port: u16,
    pub connections: Arc<AtomicUsize>,
    pub requests: Arc<AtomicUsize>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    pub fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

pub fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let files: Arc<HashMap<String, Vec<u8>>> = Arc::new(files.into_iter().collect());
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(AtomicUsize::new(0));
    let inflight = Arc::new(AtomicUsize::new(0));
    let (opened, asked) = (Arc::clone(&connections), Arc::clone(&requests));
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                break;
            };
            opened.fetch_add(1, Ordering::SeqCst);
            let (files, asked, inflight) = (
                Arc::clone(&files),
                Arc::clone(&asked),
                Arc::clone(&inflight),
            );
            thread::spawn(move || answer(stream, &files, &asked, &inflight));
        }
    });
    Server {
        port,
        connections,
        requests,
    }
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

fn answer(
    stream: TcpStream,
    files: &HashMap<String, Vec<u8>>,
    requests: &AtomicUsize,
    inflight: &AtomicUsize,
) {
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
        requests.fetch_add(1, Ordering::SeqCst);
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
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
        let (chunked, path) = route(path, "/chunked");
        let (whole, path) = route(path, "/whole");
        let (close, path) = route(path, "/close");
        let (slow, path) = route(path, "/slow");
        let _meeting = slow.then(|| Meeting::new(inflight));
        let Some(data) = files.get(path) else {
            let head = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            if writer.write_all(head.as_bytes()).is_err() {
                return;
            }
            continue;
        };
        let message = message(data, range.filter(|_| !whole), chunked, close);
        if writer.write_all(&message).is_err() || close {
            return;
        }
    }
}

fn message(data: &[u8], range: Option<(usize, usize)>, chunked: bool, close: bool) -> Vec<u8> {
    let (status, extra, body) = match range {
        Some((start, _)) if start >= data.len() => (
            "416 Range Not Satisfiable",
            format!("Content-Range: bytes */{}\r\n", data.len()),
            &data[0..0],
        ),
        Some((start, end)) => {
            let end = (end + 1).min(data.len());
            (
                "206 Partial Content",
                format!(
                    "Content-Range: bytes {start}-{}/{}\r\n",
                    end - 1,
                    data.len()
                ),
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
