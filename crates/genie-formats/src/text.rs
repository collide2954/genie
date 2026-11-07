use std::io::BufRead;

use crate::Error;

pub(crate) fn lines(
    mut reader: impl BufRead,
    mut each: impl FnMut(u64, &str) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut bytes = Vec::new();
    let mut number = 0;
    loop {
        bytes.clear();
        if reader.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(());
        }
        number += 1;
        let text = String::from_utf8_lossy(&bytes);
        let line = text.trim_end_matches(['\n', '\r']);
        if !line.is_empty() {
            each(number, line)?;
        }
    }
}
