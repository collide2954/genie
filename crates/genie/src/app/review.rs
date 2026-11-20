//! The review of the variants of a track: a bar under the tracks that goes from variant to
//! variant of the file and marks each one true, false or not sure, with a note.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use genie_core::review::{Key, Review, Reviews, Verdict};
use genie_core::variant::Variant;
use genie_core::{Region, Span, grouped};
use genie_formats::bgzf::{self, Bgzf};
use genie_formats::vcf;
use iced::advanced::widget::{operate, operation::focusable};
use iced::widget::{button, container, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length, Task};

use super::find::{BAR_HEIGHT, small};
use super::{Genie, Message, Session};
use crate::card;
use crate::chrome;
use crate::icon::Icon;
use crate::location::Location;
use crate::paths::{self, file_name};
use crate::track::{Content, Track, VariantFile, is_bcf, is_compressed};
use crate::{font, style};

/// The width of the view around a variant in review when the view is too wide to show its
/// variants.
const AROUND: u64 = 200;
pub(super) const NOTE: &str = "review-note";
const NOTE_WIDTH: f32 = 200.0;
const PLACE_WIDTH: f32 = 230.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Reviewing {
    /// Opens the review of the variants of a track, or closes it when it is open for the track.
    Open(u64),
    Close,
    /// Marks the variant in review, or removes its mark.
    Mark(Option<Verdict>),
    /// The text of the note field.
    Note(String),
    /// Enter in the note field: the field lets the keys go.
    Noted,
    /// Goes to the next or the previous variant of the track.
    Step(bool),
    Export,
    ExportPicked(Option<PathBuf>),
    Exported(Result<String, String>),
}

/// The review of the variants of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Reviewer {
    pub track: u64,
    /// The variant in review: its contig in view and its start.
    pub current: Option<(usize, u64)>,
    /// The note that somebody typed for the variant in review. Without it, the field shows the
    /// note of the mark of the variant.
    pub draft: Option<String>,
}

/// The variant of a track at a place, in all of its loaded data, also when the filter hides
/// it.
fn variant_at(track: &Track, contig: usize, position: u64) -> Option<&Variant> {
    let (loaded, data) = track.slot.loaded.as_ref()?;
    let Content::Variants(calls) = &data.content else {
        return None;
    };
    let found = (*loaded == contig).then_some(&calls.variants)?;
    found
        .iter()
        .find(|variant| variant.span.start() == position)
}

/// Whether genie can write a copy of the file of a track with its review: a VCF file on the
/// computer.
fn can_copy(track: &Track) -> bool {
    let location = track.source.data();
    track.source.has_variants() && !location.is_remote() && !is_bcf(&location.name())
}

/// The words of the variant in review, for example "chr1:12,346 A > G rs123".
fn place_words(session: &Session, track: &Track, current: Option<(usize, u64)>) -> String {
    let Some((contig, position)) = current else {
        return "Finding a variant…".to_owned();
    };
    let name = &session.reference.assembly.contigs()[contig].name;
    let place = format!("{name}:{}", grouped(position + 1));
    match variant_at(track, contig, position) {
        Some(variant) if variant.id.is_empty() => format!("{place} {}", card::change(variant)),
        Some(variant) => format!("{place} {} {}", card::change(variant), variant.id),
        None => place,
    }
}

async fn save_review(name: String, copy: bool) -> Option<PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new()
        .set_title("Export the review")
        .set_file_name(format!("{name}.tsv"))
        .add_filter("Tab-separated values", &["tsv"]);
    if copy {
        dialog = dialog.add_filter("Copy of the VCF file", &["vcf", "gz"]);
    }
    dialog.save_file().await.map(|file| file.path().to_owned())
}

/// Whether an export goes to a copy of the VCF file: a name that ends in .vcf or .gz.
fn is_copy(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("vcf") || extension.eq_ignore_ascii_case("gz")
        })
}

/// Writes a copy of a VCF file with its review: with bgzip when the name of the copy ends in
/// .gz, else as plain text.
pub(super) fn copy_reviewed(
    location: &Location,
    path: &Path,
    reviews: &Reviews,
) -> Result<String, String> {
    if location.path() == Some(path) {
        return Err("genie does not write over the open file. Pick another name.".to_owned());
    }
    let file = location.open()?;
    let reader: Box<dyn BufRead> = if is_compressed(&location.name()) {
        Box::new(Bgzf::new(file))
    } else {
        Box::new(BufReader::new(file))
    };
    let failed = |error: std::io::Error| format!("The copy could not be saved. {error}");
    let out = BufWriter::new(File::create(path).map_err(failed)?);
    let invalid = |error: genie_formats::Error| format!("The copy could not be saved. {error}");
    let marked = if is_compressed(&file_name(path)) {
        let mut writer = bgzf::Writer::new(out);
        let marked = vcf::reviewed(reader, &mut writer, reviews).map_err(invalid)?;
        let mut out = writer.finish().map_err(failed)?;
        out.flush().map_err(failed)?;
        marked
    } else {
        vcf::reviewed(reader, out, reviews).map_err(invalid)?
    };
    let variants = if marked == 1 { "variant" } else { "variants" };
    Ok(format!(
        "Saved {} with {marked} marked {variants}.",
        file_name(path)
    ))
}

impl Genie {
    pub(super) fn reviewing(&mut self, reviewing: Reviewing) -> Task<Message> {
        match reviewing {
            Reviewing::Open(id) => return self.open_review(id),
            Reviewing::Close => self.review = None,
            Reviewing::Mark(verdict) => self.mark_review(verdict),
            Reviewing::Note(text) => self.note(text),
            Reviewing::Noted => return operate(focusable::unfocus()),
            Reviewing::Step(forward) => return self.step_review(forward),
            Reviewing::Export => return self.export_review(),
            Reviewing::ExportPicked(Some(path)) => return self.write_review(path),
            Reviewing::ExportPicked(None) => {}
            Reviewing::Exported(Ok(words)) => return self.inform(words),
            Reviewing::Exported(Err(error)) => self.warn(error),
        }
        Task::none()
    }

    /// Starts the review of a track at its first variant from the left edge of the view.
    fn open_review(&mut self, id: u64) -> Task<Message> {
        self.close_menus();
        if self
            .review
            .as_ref()
            .is_some_and(|review| review.track == id)
        {
            self.review = None;
            return Task::none();
        }
        let Some(session) = &self.session else {
            return Task::none();
        };
        let Some(track) = session.tracks.iter().find(|track| track.id == id) else {
            return Task::none();
        };
        if !track.source.has_variants() {
            return Task::none();
        }
        if track.lift.is_some() {
            let words = "genie cannot review the variants of a lifted track.";
            return self.inform(words.to_owned());
        }
        let start = session.destination().visible().start();
        self.review = Some(Reviewer {
            track: id,
            current: None,
            draft: None,
        });
        self.jump_at(id, true, start.saturating_sub(1))
    }

    /// Goes to the next or the previous variant from the variant in review.
    pub(super) fn step_review(&mut self, forward: bool) -> Task<Message> {
        let Some(review) = &mut self.review else {
            return Task::none();
        };
        let id = review.track;
        let from = review.current.map(|(_, position)| position);
        review.draft = None;
        let Some(position) = from.or_else(|| self.jump_from()) else {
            return Task::none();
        };
        self.jump_at(id, forward, position)
    }

    /// The place to show for a variant that the review goes to: the view keeps its width, or
    /// shows 200 bp around the variant when it is too wide to show variants.
    pub(super) fn review_span(&self, id: u64, span: Region) -> Option<Span> {
        let review = self.review.as_ref().filter(|review| review.track == id)?;
        let session = self.session.as_ref()?;
        let wide = session.destination().span() > VariantFile::SPAN as f64;
        (review.track == id && wide).then(|| {
            let start = span.start().saturating_sub(AROUND / 2);
            Span::Region(Region::between(start, span.start() + AROUND / 2))
        })
    }

    /// Takes the variant that a jump found as the variant in review.
    pub(super) fn reviewed_at(&mut self, id: u64, contig: usize, span: Region) {
        if let Some(review) = self.review.as_mut().filter(|review| review.track == id) {
            review.current = Some((contig, span.start()));
            review.draft = None;
        }
    }

    /// The key of the variant in review, with its ID and its mark.
    fn in_review(&self) -> Option<(u64, Key, String, Option<Review>)> {
        let review = self.review.as_ref()?;
        let (contig, position) = review.current?;
        let session = self.session.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == review.track)?;
        let variant = variant_at(track, contig, position)?;
        let key = track.review_key(contig, variant)?;
        let mark = track.reviews.get(&key).cloned();
        Some((track.id, key, variant.id.clone(), mark))
    }

    fn set_review(&mut self, id: u64, key: Key, review: Option<Review>) {
        self.on_session(|session| {
            if let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) {
                Arc::make_mut(&mut track.reviews).set(key, review);
                track.refilter();
            }
        });
    }

    /// Marks the variant in review with a verdict and the note of the field, or removes its
    /// mark.
    pub(super) fn mark_review(&mut self, verdict: Option<Verdict>) {
        let Some((id, key, variant, old)) = self.in_review() else {
            if self.review.is_some() {
                self.warn("Wait until the variant shows to mark it.".to_owned());
            }
            return;
        };
        let draft = self.review.as_ref().and_then(|review| review.draft.clone());
        let note = draft
            .or_else(|| old.map(|old| old.note))
            .unwrap_or_default();
        let review = verdict.map(|verdict| Review {
            verdict,
            id: variant,
            note: note.trim().to_owned(),
        });
        self.set_review(id, key, review);
    }

    /// Keeps the text of the note field, and gives it to the mark of the variant in review.
    fn note(&mut self, text: String) {
        let Some(review) = &mut self.review else {
            return;
        };
        let note = text.trim().to_owned();
        review.draft = Some(text);
        if let Some((id, key, _, Some(old))) = self.in_review() {
            self.set_review(id, key, Some(Review { note, ..old }));
        }
    }

    fn export_review(&mut self) -> Task<Message> {
        let found = self.review.as_ref().and_then(|review| {
            let session = self.session.as_ref()?;
            session.tracks.iter().find(|track| track.id == review.track)
        });
        let Some(track) = found else {
            return Task::none();
        };
        if track.reviews.is_empty() {
            return self.inform("No variant has a mark yet.".to_owned());
        }
        let name = format!("{} review", track.name);
        Task::perform(save_review(name, can_copy(track)), |path| {
            Message::Review(Reviewing::ExportPicked(path))
        })
    }

    fn write_review(&mut self, path: PathBuf) -> Task<Message> {
        let found = self.review.as_ref().and_then(|review| {
            let session = self.session.as_ref()?;
            session.tracks.iter().find(|track| track.id == review.track)
        });
        let Some(track) = found else {
            return Task::none();
        };
        let reviews = Arc::clone(&track.reviews);
        if is_copy(&path) && can_copy(track) {
            let location = track.source.data().clone();
            return Task::perform(
                async move { copy_reviewed(&location, &path, &reviews) },
                |result| Message::Review(Reviewing::Exported(result)),
            );
        }
        let result = paths::replace(&path, &reviews.tsv())
            .map(|()| format!("Saved {}.", file_name(&path)))
            .map_err(|error| format!("The review could not be saved. {error}"));
        self.reviewing(Reviewing::Exported(result))
    }

    pub(super) fn review_bar<'a>(&'a self, session: &'a Session) -> Option<Element<'a, Message>> {
        let review = self.review.as_ref()?;
        let track = session
            .tracks
            .iter()
            .find(|track| track.id == review.track)?;
        let mark = review.current.and_then(|(contig, position)| {
            let variant = variant_at(track, contig, position)?;
            track.review_of(contig, variant)
        });
        let below = tooltip::Position::Bottom;
        let verdict = |verdict: Verdict, key: char| {
            let chosen = mark.is_some_and(|mark| mark.verdict == verdict);
            let label = match verdict {
                Verdict::True => "True",
                Verdict::False => "False",
                Verdict::Unsure => "Not sure",
            };
            let choice = button(text(label).size(11))
                .padding([2, 7])
                .style(if chosen { style::chosen } else { style::ghost })
                .on_press(Message::Review(Reviewing::Mark(Some(verdict))));
            chrome::tip(choice, format!("Mark {} ({key})", verdict.label()), below)
        };
        let clear = button(text("Clear").size(11))
            .padding([2, 7])
            .style(style::ghost)
            .on_press_maybe(mark.map(|_| Message::Review(Reviewing::Mark(None))));
        let note = review
            .draft
            .as_deref()
            .or_else(|| mark.map(|mark| mark.note.as_str()))
            .unwrap_or_default();
        let field = text_input("Note", note)
            .id(NOTE)
            .size(12)
            .padding([3, 8])
            .width(NOTE_WIDTH)
            .style(style::locus)
            .on_input(|text| Message::Review(Reviewing::Note(text)))
            .on_submit(Message::Review(Reviewing::Noted));
        let count = track.reviews.len();
        let marked = format!("{count} marked");
        let export = button(text("Export…").size(11))
            .padding([2, 7])
            .style(style::ghost)
            .on_press(Message::Review(Reviewing::Export));
        let bar = row![
            text("Review").size(11).style(style::muted),
            text(place_words(session, track, review.current))
                .size(11)
                .font(font::MONO)
                .width(PLACE_WIDTH)
                .wrapping(text::Wrapping::None),
            verdict(Verdict::True, 'T'),
            verdict(Verdict::False, 'F'),
            verdict(Verdict::Unsure, 'U'),
            clear,
            field,
            text(marked)
                .size(11)
                .style(style::muted)
                .width(Length::Fill)
                .wrapping(text::Wrapping::None),
            chrome::tip(
                small(Icon::Back, Message::Review(Reviewing::Step(false))),
                "Previous variant (P)",
                below
            ),
            chrome::tip(
                small(Icon::Forward, Message::Review(Reviewing::Step(true))),
                "Next variant (N)",
                below
            ),
            chrome::tip(
                export,
                "Save the marks as a TSV file or as a copy of the VCF file",
                below
            ),
            chrome::tip(
                small(Icon::Close, Message::Review(Reviewing::Close)),
                "Close the review",
                below
            ),
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Center);
        Some(
            container(bar)
                .id("review-bar")
                .padding([0, 8])
                .height(BAR_HEIGHT)
                .clip(true)
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use genie_core::review::{Key, Review, Reviews, Verdict};
    use genie_formats::bgzf::Bgzf;

    use super::copy_reviewed;
    use crate::location::Location;
    use crate::scratch::Scratch;

    /// A copy has the marks in its INFO, with bgzip for a name that ends in .gz, and genie
    /// does not write over the open file.
    #[test]
    fn copies_of_vcf_files_carry_the_marks_and_keep_the_file() {
        let scratch = Scratch::new("review-copy");
        let text = "##fileformat=VCFv4.2\n#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\n\
                    chr1\t21\trs20\tA\tG\t.\tPASS\tDP=12\n";
        let file = scratch.file("calls.vcf", text);
        let source = Location::File(file.clone());
        let mut reviews = Reviews::default();
        let key = Key {
            contig: "chr1".to_owned(),
            position: 20,
            reference: "A".to_owned(),
            alternates: "G".to_owned(),
        };
        let review = Review {
            verdict: Verdict::True,
            id: "rs20".to_owned(),
            note: String::new(),
        };
        reviews.set(key, Some(review));
        let plain = scratch.file("copy.vcf", "");
        let saved = copy_reviewed(&source, &plain, &reviews);
        assert_eq!(
            saved,
            Ok("Saved copy.vcf with 1 marked variant.".to_owned())
        );
        let copy = std::fs::read_to_string(&plain).unwrap();
        assert!(copy.contains("PASS\tDP=12;REVIEW=true"), "{copy}");
        let packed = scratch.file("copy.vcf.gz", "");
        copy_reviewed(&source, &packed, &reviews).unwrap();
        let mut unpacked = String::new();
        let bytes = std::fs::read(&packed).unwrap();
        Bgzf::new(Cursor::new(bytes))
            .read_to_string(&mut unpacked)
            .unwrap();
        assert_eq!(unpacked, copy);
        assert!(copy_reviewed(&source, &file, &reviews).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    }
}
