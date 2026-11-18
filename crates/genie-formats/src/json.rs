//! A small JSON reader for the answers of web services, such as the BLAT server of UCSC.

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    List(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value of a key of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn list(&self) -> Option<&[Json]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// A text, or a number as text.
    pub fn text(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Number(number) => Some(number.to_string()),
            _ => None,
        }
    }
}

/// The deepest lists and objects in lists and objects that genie reads.
const DEPTH: usize = 64;

/// Reads a JSON text, or says why it is not valid.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = reader.value(0)?;
    reader.space();
    if reader.at < reader.bytes.len() {
        return Err(reader.wrong("there is more text after the value"));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn wrong(&self, what: &str) -> String {
        format!("The JSON text is not valid at byte {}: {what}.", self.at)
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > DEPTH {
            return Err(self.wrong("the lists are too deep"));
        }
        self.space();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.text().map(Json::Text),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.wrong("a value is missing")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.wrong("a word is not known"))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|text| text.parse::<f64>().ok())
            .map(Json::Number)
            .ok_or_else(|| self.wrong("a number is not valid"))
    }

    fn hex(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.wrong("an escape is not valid"))?;
        self.at += 4;
        Ok(digits)
    }

    /// A text in quotes. A pair of surrogate escapes gives one letter.
    fn text(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut bytes = Vec::new();
        loop {
            let byte = self.peek().ok_or_else(|| self.wrong("a text has no end"))?;
            self.at += 1;
            match byte {
                b'"' => return Ok(String::from_utf8_lossy(&bytes).into_owned()),
                b'\\' => {
                    let escape = self.peek().ok_or_else(|| self.wrong("a text has no end"))?;
                    self.at += 1;
                    let letter = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return Err(self.wrong("an escape is not known")),
                    };
                    let mut buffer = [0; 4];
                    bytes.extend_from_slice(letter.encode_utf8(&mut buffer).as_bytes());
                }
                _ => bytes.push(byte),
            }
        }
    }

    fn unicode(&mut self) -> Result<char, String> {
        let high = self.hex()?;
        if (0xD800..0xDC00).contains(&high) && self.bytes[self.at..].starts_with(b"\\u") {
            self.at += 2;
            let low = self.hex()?;
            let code = 0x10000 + ((high - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
            return Ok(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        Ok(char::from_u32(high).unwrap_or(char::REPLACEMENT_CHARACTER))
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::List(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::List(items));
                }
                _ => return Err(self.wrong("a list needs a comma or its end")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut entries = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return Err(self.wrong("a key is missing"));
            }
            let key = self.text()?;
            self.space();
            if self.peek() != Some(b':') {
                return Err(self.wrong("a colon is missing"));
            }
            self.at += 1;
            entries.push((key, self.value(depth + 1)?));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.wrong("an object needs a comma or its end")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Json, parse};

    #[test]
    fn objects_lists_texts_and_numbers() {
        let found =
            parse(r#" {"track": "blat", "n": [1, -2.5e2, true, null], "s": "a\"b\\c\né"} "#)
                .unwrap();
        assert_eq!(found.get("track"), Some(&Json::Text("blat".to_owned())));
        assert_eq!(
            found.get("n").and_then(Json::list),
            Some(
                &[
                    Json::Number(1.0),
                    Json::Number(-250.0),
                    Json::Bool(true),
                    Json::Null
                ][..]
            )
        );
        assert_eq!(
            found.get("s").and_then(Json::text).as_deref(),
            Some("a\"b\\c\né")
        );
        assert_eq!(Json::Number(24.0).text().as_deref(), Some("24"));
    }

    #[test]
    fn broken_texts_say_so() {
        for text in [
            "",
            "{",
            "[1,",
            "{\"a\" 1}",
            "tru",
            "\"open",
            "[1] 2",
            "{\"a\":01x}",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn deep_nesting_stops_without_a_crash() {
        let deep = "[".repeat(10_000);
        assert!(parse(&deep).is_err());
    }
}
