use std::sync::Arc;

use genie_core::{Locus, Region, Span, codon, grouped};
use genie_formats::fasta;
use iced::widget::column;
use iced::{Element, Point, Task};

use crate::card;
use crate::menu;
use crate::reference::Reference;

use super::{Copying, Genie, Message};

pub(super) const MENU_HEIGHT: f32 = 53.0;
const SEQUENCE_LIMIT: u64 = 1_000_000;
const TOO_LONG: &str = "genie copies up to 1 Mb of sequence. Zoom in, or choose a smaller region.";

impl Genie {
    pub(super) fn copy_text(&self) -> Option<String> {
        if let Some(card) = &self.pinned {
            return Some(card::plain(card));
        }
        self.session.as_ref().map(super::Session::locus)
    }

    pub(super) fn copy(&mut self) -> Task<Message> {
        let Some(text) = self.copy_text() else {
            return Task::none();
        };
        let notice = if self.pinned.is_some() {
            "Copied the card text.".to_owned()
        } else {
            format!("Copied {text}.")
        };
        Task::batch([iced::clipboard::write(text), self.inform(notice)])
    }

    pub(super) fn open_copy_menu(&mut self, region: Option<u64>, at: Point) {
        self.close_menus();
        self.copy_menu = Some((region, at));
    }

    pub(super) fn copy_menu(&self) -> Option<Element<'_, Message>> {
        let (region, at) = self.copy_menu?;
        let content = column![
            menu::item(
                "Copy locus",
                Message::Copy(Copying::Locus(region)),
                false,
                true
            ),
            menu::item(
                "Copy sequence",
                Message::Copy(Copying::Sequence(region)),
                false,
                true
            ),
        ]
        .spacing(1);
        Some(menu::card(content, at, self.window, MENU_HEIGHT))
    }

    fn target(&self, region: Option<u64>) -> Option<(usize, Region)> {
        let session = self.session.as_ref()?;
        match region {
            None => Some((session.contig, session.viewport.visible())),
            Some(id) => session
                .marks
                .all()
                .iter()
                .find(|mark| mark.id == id)
                .map(|mark| (mark.contig, mark.span)),
        }
    }

    pub(super) fn copying(&mut self, copying: Copying) -> Task<Message> {
        self.copy_menu = None;
        match copying {
            Copying::Locus(region) => {
                let (Some(session), Some((contig, span))) = (&self.session, self.target(region))
                else {
                    return Task::none();
                };
                let text = Locus {
                    name: session.reference.assembly.contigs()[contig].name.clone(),
                    span: Span::Region(span),
                }
                .to_string();
                Task::batch([
                    iced::clipboard::write(text.clone()),
                    self.inform(format!("Copied {text}.")),
                ])
            }
            Copying::Sequence(region) => {
                let (Some(session), Some((contig, span))) = (&self.session, self.target(region))
                else {
                    return Task::none();
                };
                if span.len() > SEQUENCE_LIMIT {
                    self.warn(TOO_LONG.to_owned());
                    return Task::none();
                }
                let reference = Arc::clone(&session.reference);
                let reverse = session.reading.reverse;
                Task::perform(
                    async move { fasta_text(&reference, contig, span, reverse) },
                    |result| Message::Copy(Copying::Fetched(result)),
                )
            }
            Copying::Fetched(Ok((text, length))) => Task::batch([
                iced::clipboard::write(text),
                self.inform(format!("Copied {} bp as FASTA.", grouped(length))),
            ]),
            Copying::Fetched(Err(error)) => {
                self.warn(format!("genie cannot read the sequence. {error}"));
                Task::none()
            }
        }
    }
}

fn fasta_text(
    reference: &Reference,
    contig: usize,
    span: Region,
    reverse: bool,
) -> Result<(String, u64), String> {
    let found = &reference.assembly.contigs()[contig];
    let span = span.clamp_to(found.length);
    let mut bases = reference.fetch(contig, span)?;
    let mut header = format!("{}:{}-{}", found.name, span.start() + 1, span.end());
    if reverse {
        bases.reverse();
        for base in &mut bases {
            *base = codon::complement(*base);
        }
        header.push_str("(-)");
    }
    Ok((fasta::record(&header, &bases), bases.len() as u64))
}

#[cfg(test)]
mod tests {
    use genie_core::Region;

    use super::fasta_text;
    use crate::scratch::Scratch;

    #[test]
    fn sequences_copy_as_fasta_on_the_strand_shown() {
        let scratch = Scratch::new("copy-fasta");
        let file = scratch.file("ref.fa", ">chr1\nAACCGGTTAC\n");
        let reference = crate::reference::open(file, None, &mut |_| {}).unwrap();
        let copy =
            |start, end, reverse| fasta_text(&reference, 0, Region::between(start, end), reverse);
        assert_eq!(
            copy(2, 6, false).unwrap(),
            (">chr1:3-6\nCCGG\n".to_owned(), 4)
        );
        assert_eq!(
            copy(0, 4, true).unwrap(),
            (">chr1:1-4(-)\nGGTT\n".to_owned(), 4)
        );
        assert_eq!(
            copy(8, 30, false).unwrap(),
            (">chr1:9-10\nAC\n".to_owned(), 2)
        );
    }
}
