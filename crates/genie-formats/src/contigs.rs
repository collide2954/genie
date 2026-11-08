use std::io::BufRead;

use crate::bed::number;
use crate::text::lines;
use crate::{Error, Reason};

pub fn sizes(reader: impl BufRead) -> Result<Vec<(String, u64)>, Error> {
    let mut rows = Vec::new();
    lines(reader, |line_number, line| {
        if line.starts_with('#') {
            return Ok(());
        }
        let mut fields = line.split('\t');
        let (Some(name), Some(length)) = (fields.next(), fields.next()) else {
            return Err(Error::Format {
                line: line_number,
                reason: Reason::MissingField,
            });
        };
        let length = number(length).map_err(|reason| Error::Format {
            line: line_number,
            reason,
        })?;
        rows.push((name.to_owned(), length));
        Ok(())
    })?;
    Ok(rows)
}

pub fn aliases(reader: impl BufRead) -> Result<Vec<Vec<String>>, Error> {
    let mut rows = Vec::new();
    lines(reader, |_, line| {
        if !line.starts_with('#') {
            let names: Vec<String> = line
                .split('\t')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect();
            if !names.is_empty() {
                rows.push(names);
            }
        }
        Ok(())
    })?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::{aliases, sizes};
    use crate::{Error, Reason};

    #[test]
    fn sizes_are_names_and_lengths() {
        let text = "chr1\t248956422\nchrM\t16569\n\n";
        assert_eq!(
            sizes(text.as_bytes()).unwrap(),
            [
                ("chr1".to_owned(), 248_956_422),
                ("chrM".to_owned(), 16_569)
            ]
        );
        for (bad, reason) in [
            ("chr1\n", Reason::MissingField),
            ("chr1\tbig\n", Reason::BadNumber),
        ] {
            let error = sizes(bad.as_bytes()).unwrap_err();
            assert!(
                matches!(error, Error::Format { line: 1, reason: found } if found == reason),
                "{bad}"
            );
        }
    }

    #[test]
    fn alias_rows_list_every_name_of_a_contig() {
        let text = "# sequenceName\talias names\tUCSC database: hg38\n\
                    chr1\t1\tCM000663.2\tNC_000001.11\n\
                    chrM\tMT\t\tNC_012920.1\n";
        assert_eq!(
            aliases(text.as_bytes()).unwrap(),
            [
                vec!["chr1", "1", "CM000663.2", "NC_000001.11"],
                vec!["chrM", "MT", "NC_012920.1"],
            ]
        );
    }
}
