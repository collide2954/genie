use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use genie_core::Region;
use genie_render::features::Mode;

pub const PORT: u16 = 60151;
const WAIT: Duration = Duration::from_millis(100);
const LONGEST: u64 = 64 << 10;

#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    Echo,
    New,
    Genome(String),
    Load {
        file: String,
        name: Option<String>,
    },
    Goto(String),
    Region {
        contig: String,
        span: Region,
        name: Option<String>,
    },
    Folder(PathBuf),
    Snapshot(Option<String>),
    Rows(Mode, Option<String>),
    Ignored,
}

pub fn parse(line: &str) -> Result<Order, String> {
    let words = words(line);
    let Some((first, rest)) = words.split_first() else {
        return Err("Give a command.".to_owned());
    };
    let needs = |what: &str| {
        rest.first()
            .cloned()
            .ok_or_else(|| format!("{first} needs {what}."))
    };
    let optional = rest.first().cloned();
    Ok(match first.to_ascii_lowercase().as_str() {
        "echo" => Order::Echo,
        "new" => Order::New,
        "genome" => Order::Genome(needs("a genome or a file")?),
        "load" => Order::Load {
            file: needs("a file")?,
            name: rest
                .iter()
                .skip(1)
                .find_map(|word| word.strip_prefix("name="))
                .map(str::to_owned),
        },
        "goto" if rest.is_empty() => return Err(format!("{first} needs a locus.")),
        "goto" => Order::Goto(rest.join(" ")),
        "region" => region(first, rest)?,
        "snapshotdirectory" => Order::Folder(PathBuf::from(needs("a folder")?)),
        "snapshot" => Order::Snapshot(optional),
        "expand" => Order::Rows(Mode::Expanded, optional),
        "squish" => Order::Rows(Mode::Squished, optional),
        "collapse" => Order::Rows(Mode::Collapsed, optional),
        "maxpanelheight" | "setsleepinterval" => Order::Ignored,
        _ => return Err(format!("genie does not know the command {first}.")),
    })
}

fn region(first: &str, rest: &[String]) -> Result<Order, String> {
    let [contig, start, end, name @ ..] = rest else {
        return Err(format!("{first} needs a contig, a start and an end."));
    };
    let number = |text: &str| text.replace(',', "").parse::<u64>().ok();
    let (Some(start), Some(end)) = (number(start), number(end)) else {
        return Err("The start and the end of a region must be numbers.".to_owned());
    };
    let start = start.saturating_sub(1);
    if end <= start {
        return Err("The end of a region must not be before its start.".to_owned());
    }
    Ok(Order::Region {
        contig: contig.clone(),
        span: Region::between(start, end),
        name: (!name.is_empty()).then(|| name.join(" ")),
    })
}

fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let (mut quoted, mut started) = (false, false);
    for letter in line.chars() {
        match letter {
            '"' => (quoted, started) = (!quoted, true),
            letter if letter.is_whitespace() && !quoted => {
                if std::mem::take(&mut started) {
                    words.push(std::mem::take(&mut word));
                }
            }
            letter => {
                word.push(letter);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

#[derive(Clone)]
pub struct Reply(Arc<Mutex<Option<Sender<String>>>>);

impl Reply {
    pub fn new() -> (Self, Receiver<String>) {
        let (sender, receiver) = mpsc::channel();
        (Self(Arc::new(Mutex::new(Some(sender)))), receiver)
    }

    pub fn send(&self, answer: impl Into<String>) {
        let sender = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(sender) = sender {
            let _ = sender.send(answer.into());
        }
    }
}

impl PartialEq for Reply {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Reply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Reply")
    }
}

pub fn listen(port: u16) -> io::Result<TcpListener> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

pub fn serve<F>(listener: &TcpListener, ask: &Arc<F>, open: impl Fn() -> bool)
where
    F: Fn(String) -> Option<String> + Send + Sync + 'static,
{
    while open() {
        match listener.accept() {
            Ok((stream, _)) => {
                let ask = Arc::clone(ask);
                thread::spawn(move || talk(stream, ask.as_ref()));
            }
            Err(_) => thread::sleep(WAIT),
        }
    }
}

fn talk(stream: TcpStream, ask: &impl Fn(String) -> Option<String>) {
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let Ok(copy) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(copy);
    let mut writer = stream;
    let mut first = true;
    loop {
        let mut bytes = Vec::new();
        match reader.by_ref().take(LONGEST).read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(read) if read as u64 == LONGEST && !bytes.ends_with(b"\n") => return,
            Ok(_) => {}
        }
        let Ok(text) = String::from_utf8(bytes) else {
            return;
        };
        let line = text.trim();
        if std::mem::take(&mut first) && is_web(line) {
            return;
        }
        if line.is_empty() {
            continue;
        }
        let Some(answer) = ask(line.to_owned()) else {
            return;
        };
        if writer.write_all(format!("{answer}\n").as_bytes()).is_err() {
            return;
        }
    }
}

fn is_web(line: &str) -> bool {
    let mut words = line.split(' ');
    match (words.next(), words.next(), words.next()) {
        (Some(method), Some(_), Some(version)) => {
            !method.is_empty()
                && method.bytes().all(|byte| byte.is_ascii_uppercase())
                && version.starts_with("HTTP/")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use std::path::PathBuf;

    use genie_core::Region;
    use genie_render::features::Mode;

    use super::{Order, Reply, listen, parse, serve};

    fn start(answer: impl Fn(String) -> Option<String> + Send + Sync + 'static) -> u16 {
        let listener = listen(0).unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || serve(&listener, &Arc::new(answer), || true));
        port
    }

    fn connect(port: u16) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }

    fn talk(port: u16, lines: &[&str]) -> Vec<String> {
        let mut stream = connect(port);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut answers = Vec::new();
        for line in lines {
            if stream.write_all(format!("{line}\n").as_bytes()).is_err() {
                break;
            }
            let mut answer = String::new();
            if reader.read_line(&mut answer).unwrap_or(0) == 0 {
                break;
            }
            answers.push(answer.trim_end().to_owned());
        }
        answers
    }

    #[test]
    fn each_line_gets_one_answer() {
        let port = start(|line| Some(format!("got {line}")));
        assert_eq!(
            talk(port, &["echo", " goto chr1:1-100\r", "new"]),
            ["got echo", "got goto chr1:1-100", "got new"]
        );
    }

    #[test]
    fn web_requests_get_no_answer() {
        let asked = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&asked);
        let port = start(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Some("OK".to_owned())
        });
        let mut stream = connect(port);
        let request = "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: text/plain\r\n\r\n\
                       load http://example.org/x.bam\n";
        stream.write_all(request.as_bytes()).unwrap();
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
        assert_eq!(rest, [] as [u8; 0]);
        assert_eq!(asked.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn connections_close_when_the_app_is_gone() {
        let port = start(|_| None);
        assert_eq!(talk(port, &["echo", "echo"]), [] as [String; 0]);
    }

    #[test]
    fn the_port_is_only_for_this_computer() {
        let listener = listen(0).unwrap();
        assert!(listener.local_addr().unwrap().ip().is_loopback());
    }

    #[test]
    fn lines_become_orders() {
        let text = |word: &str| word.to_owned();
        assert_eq!(parse("echo"), Ok(Order::Echo));
        assert_eq!(parse("new"), Ok(Order::New));
        assert_eq!(parse("genome hg38"), Ok(Order::Genome(text("hg38"))));
        assert_eq!(
            parse("GOTO chr1:100-200"),
            Ok(Order::Goto(text("chr1:100-200")))
        );
        assert_eq!(
            parse("goto chr1:100-200  chr2:5-9"),
            Ok(Order::Goto(text("chr1:100-200 chr2:5-9")))
        );
        assert_eq!(
            parse("load \"/data/my reads.bam\" index=x.bai name=Tumor"),
            Ok(Order::Load {
                file: text("/data/my reads.bam"),
                name: Some(text("Tumor")),
            })
        );
        assert_eq!(
            parse("region chr1 1,001 2000 promoter A"),
            Ok(Order::Region {
                contig: text("chr1"),
                span: Region::between(1_000, 2_000),
                name: Some(text("promoter A")),
            })
        );
        assert_eq!(
            parse("region chrM 0 5"),
            Ok(Order::Region {
                contig: text("chrM"),
                span: Region::between(0, 5),
                name: None,
            })
        );
        assert_eq!(
            parse("snapshotDirectory /tmp/shots"),
            Ok(Order::Folder(PathBuf::from("/tmp/shots")))
        );
        assert_eq!(parse("snapshot"), Ok(Order::Snapshot(None)));
        assert_eq!(
            parse("snapshot brca1.png"),
            Ok(Order::Snapshot(Some(text("brca1.png"))))
        );
        assert_eq!(
            parse("squish genes.bed"),
            Ok(Order::Rows(Mode::Squished, Some(text("genes.bed"))))
        );
        assert_eq!(parse("expand"), Ok(Order::Rows(Mode::Expanded, None)));
        assert_eq!(parse("collapse"), Ok(Order::Rows(Mode::Collapsed, None)));
        assert_eq!(parse("maxPanelHeight 500"), Ok(Order::Ignored));
        assert_eq!(parse("setSleepInterval 0"), Ok(Order::Ignored));
    }

    #[test]
    fn wrong_lines_say_why() {
        let wrong = |line: &str| parse(line).unwrap_err();
        assert_eq!(wrong("fly away"), "genie does not know the command fly.");
        assert_eq!(wrong("goto"), "goto needs a locus.");
        assert_eq!(wrong("load"), "load needs a file.");
        assert_eq!(wrong("genome"), "genome needs a genome or a file.");
        assert_eq!(
            wrong("snapshotDirectory"),
            "snapshotDirectory needs a folder."
        );
        assert_eq!(
            wrong("region chr1 10"),
            "region needs a contig, a start and an end."
        );
        assert_eq!(
            wrong("region chr1 x 20"),
            "The start and the end of a region must be numbers."
        );
        assert_eq!(
            wrong("region chr1 20 10"),
            "The end of a region must not be before its start."
        );
    }

    #[test]
    fn replies_go_back_once() {
        let (reply, answer) = Reply::new();
        let copy = reply.clone();
        assert_eq!(reply, copy);
        copy.send("OK");
        reply.send("again");
        assert_eq!(answer.try_recv().as_deref(), Ok("OK"));
        assert!(answer.try_recv().is_err());
    }

    #[test]
    fn the_server_stops_when_asked() {
        let listener = listen(0).unwrap();
        let open = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&open);
        let server = thread::spawn(move || {
            serve(&listener, &Arc::new(|_| Some("OK".to_owned())), || {
                flag.load(Ordering::SeqCst)
            });
        });
        open.store(false, Ordering::SeqCst);
        server.join().unwrap();
    }
}
