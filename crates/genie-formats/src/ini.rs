use crate::{Error, Reason};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl Document {
    pub fn sections<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a Section> {
        self.sections
            .iter()
            .filter(move |section| section.name == name)
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections(name).next()
    }
}

impl Section {
    pub fn values<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a str> {
        self.entries
            .iter()
            .filter(move |(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.values(key).next()
    }
}

pub fn parse(text: &str) -> Result<Document, Error> {
    let mut document = Document::default();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let fail = |reason| Error::Format {
            line: index as u64 + 1,
            reason,
        };
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let name = name.trim();
            if name.is_empty() {
                return Err(fail(Reason::BadLine));
            }
            document.sections.push(Section {
                name: name.to_owned(),
                entries: Vec::new(),
            });
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(fail(Reason::BadLine));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(fail(Reason::BadLine));
        }
        let Some(section) = document.sections.last_mut() else {
            return Err(fail(Reason::NoSection));
        };
        section
            .entries
            .push((key.to_owned(), value.trim().to_owned()));
    }
    Ok(document)
}

pub fn write(document: &Document) -> String {
    document
        .sections
        .iter()
        .map(|section| {
            let mut lines = vec![format!("[{}]", section.name)];
            lines.extend(
                section
                    .entries
                    .iter()
                    .map(|(key, value)| format!("{key} = {value}")),
            );
            lines.join("\n") + "\n"
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{Document, Section, parse, write};
    use crate::{Error, Reason};

    fn failure(text: &str) -> (u64, Reason) {
        match parse(text) {
            Err(Error::Format { line, reason }) => (line, reason),
            other => panic!("expected a format error, got {other:?}"),
        }
    }

    #[test]
    fn parses_sections_and_entries() {
        let document = parse("[recent]\nfile = /data/hg38.2bit\nfile = C:\\ref.fa\n").unwrap();
        let recent = document.section("recent").unwrap();
        assert_eq!(
            recent.values("file").collect::<Vec<_>>(),
            ["/data/hg38.2bit", "C:\\ref.fa"]
        );
    }

    #[test]
    fn keeps_repeated_sections_in_order() {
        let document = parse("[track]\nname = a\n[track]\nname = b\n").unwrap();
        let names: Vec<_> = document
            .sections("track")
            .filter_map(|section| section.value("name"))
            .collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn skips_comments_and_blank_lines() {
        let document = parse("# genie\n\n; note\n[view]\n\nlocus = chr1:1-100\n").unwrap();
        assert_eq!(
            document.section("view").unwrap().value("locus"),
            Some("chr1:1-100")
        );
    }

    #[test]
    fn values_keep_equals_signs_and_inner_spaces() {
        let document = parse("[a]\nkey =  b = c d  \n").unwrap();
        assert_eq!(document.section("a").unwrap().value("key"), Some("b = c d"));
    }

    #[test]
    fn rejects_entries_outside_sections_and_other_lines() {
        assert_eq!(failure("file = x\n"), (1, Reason::NoSection));
        assert_eq!(failure("[a]\njust text\n"), (2, Reason::BadLine));
        assert_eq!(failure("[a]\n = x\n"), (2, Reason::BadLine));
        assert_eq!(failure("[]\n"), (1, Reason::BadLine));
    }

    #[test]
    fn written_documents_parse_back() {
        let document = Document {
            sections: vec![
                Section {
                    name: "recent".to_owned(),
                    entries: vec![("file".to_owned(), "/a b/ref.fa".to_owned())],
                },
                Section {
                    name: "view".to_owned(),
                    entries: vec![("locus".to_owned(), "chr2".to_owned())],
                },
            ],
        };
        let text = write(&document);
        assert_eq!(
            text,
            "[recent]\nfile = /a b/ref.fa\n\n[view]\nlocus = chr2\n"
        );
        assert_eq!(parse(&text).unwrap(), document);
    }
}
