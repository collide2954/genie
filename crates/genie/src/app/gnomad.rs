//! gnomAD frequencies in the open card of a variant: one question to the gnomAD API for each
//! allele, when the gnomAD setting is on. genie keeps the answers.

use std::collections::HashMap;

use genie_core::grouped;
use genie_core::variant::Variant;
use genie_formats::gnomad::{self, Count, Frequencies};
use iced::Task;

use super::{Genie, Message};
use crate::card::{Card, Detail, Gnomad};
use crate::reference::Reference;
use crate::settings::Setting;

/// The most alleles of a variant that genie asks gnomAD about.
const ALLELES: usize = 3;
/// The lengths of chromosome 1 in hg38 and in hg19, which tell a human reference that has no
/// genome name.
const GRCH38: u64 = 248_956_422;
const GRCH37: u64 = 249_250_621;

/// The answers of gnomAD about the alleles of a variant, by their gnomAD names.
pub type Answers = Vec<(String, Result<Option<Frequencies>, String>)>;

#[derive(Debug, Clone, PartialEq)]
pub enum Gnomading {
    /// Turns the gnomAD setting on, and asks about the variant of the open card.
    TurnOn,
    /// The answers about the alleles of a variant, with the number of the question and the data
    /// set.
    Answered(u64, &'static str, Answers),
}

/// A variant as a card shows it: its contig, its start and its reference allele.
type Key = (String, u64, String);

/// The gnomAD part of the open card: its variant, the data set and its name, the alleles with
/// their gnomAD names, and what the part shows.
#[derive(Debug, Clone)]
struct Shown {
    key: Key,
    dataset: &'static str,
    name: &'static str,
    alleles: Vec<(String, String)>,
    part: Gnomad,
}

/// The gnomAD answers that genie has, and the gnomAD part of the open card.
#[derive(Debug, Default)]
pub(super) struct Lookups {
    /// The answer for each gnomAD name of a variant in a data set.
    answers: HashMap<(&'static str, String), Option<Frequencies>>,
    /// The number of the last question.
    asked: u64,
    shown: Option<Shown>,
}

fn variant_of(card: &Card) -> Option<(Key, &Variant)> {
    let Detail::Variant { variant, .. } = &card.detail else {
        return None;
    };
    let key = (
        card.contig.clone(),
        variant.span.start(),
        variant.reference.clone(),
    );
    Some((key, variant))
}

/// The gnomAD data set of a human reference and its name: by the name of its genome, or else by
/// the length of its chromosome 1.
fn dataset(reference: &Reference) -> Option<(&'static str, &'static str)> {
    if let Some(genome) = &reference.genome {
        return gnomad::dataset(genome);
    }
    let contig = reference.assembly.find("chr1")?;
    match reference.assembly.contigs()[contig].length {
        GRCH38 => gnomad::dataset("hg38"),
        GRCH37 => gnomad::dataset("hg19"),
        _ => None,
    }
}

/// The frequency and the counts of a group of alleles, for example "1.23 % · 12 of 974".
fn counted(count: Count) -> String {
    let share = count
        .frequency()
        .map_or_else(|| "–".to_owned(), gnomad::percent);
    format!(
        "{share} · {} of {}",
        grouped(count.alternate),
        grouped(count.total)
    )
}

/// The line of the frequencies of an allele, for example
/// "gnomAD v4.1: 1.23 % · 12 of 974 alleles · 1 homozygote".
fn line(name: &str, found: &Frequencies) -> String {
    let homozygotes = match found.homozygotes {
        0 => String::new(),
        1 => " · 1 homozygote".to_owned(),
        count => format!(" · {} homozygotes", grouped(count)),
    };
    format!("{name}: {} alleles{homozygotes}", counted(found.all))
}

impl Lookups {
    /// The gnomAD part of an open card, when the card shows the variant of the last question.
    pub(super) fn part(&self, card: &Card) -> Option<&Gnomad> {
        let (key, _) = variant_of(card)?;
        self.shown
            .as_ref()
            .filter(|shown| shown.key == key)
            .map(|shown| &shown.part)
    }

    /// The part of a card from the answers that genie has, or None when one is missing.
    fn found(&self, shown: &Shown) -> Option<Gnomad> {
        let several = shown.alleles.len() > 1;
        let (mut lines, mut groups) = (Vec::new(), Vec::new());
        for (allele, id) in &shown.alleles {
            let answer = self.answers.get(&(shown.dataset, id.clone()))?;
            let name = if several {
                format!("{} {allele}", shown.name)
            } else {
                shown.name.to_owned()
            };
            match answer {
                None => lines.push(format!("{name}: not in gnomAD")),
                Some(found) => {
                    lines.push(line(&name, found));
                    if !several {
                        groups = found
                            .groups
                            .iter()
                            .map(|(group, count)| ((*group).to_owned(), counted(*count)))
                            .collect();
                    }
                }
            }
        }
        Some(Gnomad::Found { lines, groups })
    }
}

impl Genie {
    pub(super) fn gnomading(&mut self, gnomading: Gnomading) -> Task<Message> {
        match gnomading {
            Gnomading::TurnOn => {
                self.settings = self.settings.apply(Setting::Gnomad(true));
                Task::batch([self.save_settings(), self.ask_gnomad()])
            }
            Gnomading::Answered(number, dataset, answers) => {
                self.gnomad_answered(number, dataset, answers);
                Task::none()
            }
        }
    }

    /// Asks gnomAD about the variant of the open card, when the card shows a variant of a human
    /// reference, genie has no answer for it, and the gnomAD setting is on. With the setting
    /// off, the card shows a button that turns it on.
    pub(super) fn ask_gnomad(&mut self) -> Task<Message> {
        let lookups = &mut self.gnomad;
        lookups.shown = None;
        let (Some(card), Some(session)) = (&self.pinned, &self.session) else {
            return Task::none();
        };
        let (Some((key, variant)), Some((dataset, name))) =
            (variant_of(card), dataset(&session.reference))
        else {
            return Task::none();
        };
        let start = variant.span.start() + 1;
        let alleles: Vec<(String, String)> = variant
            .alternates
            .iter()
            .take(ALLELES)
            .filter_map(|allele| {
                let id = gnomad::variant_id(&card.contig, start, &variant.reference, allele)?;
                Some((allele.clone(), id))
            })
            .collect();
        if alleles.is_empty() {
            return Task::none();
        }
        let mut shown = Shown {
            key,
            dataset,
            name,
            alleles,
            part: Gnomad::Off,
        };
        if self.settings.gnomad
            && let Some(found) = lookups.found(&shown)
        {
            shown.part = found;
        } else if self.settings.gnomad {
            lookups.asked += 1;
            let number = lookups.asked;
            let missing: Vec<String> = shown
                .alleles
                .iter()
                .map(|(_, id)| id.clone())
                .filter(|id| !lookups.answers.contains_key(&(dataset, id.clone())))
                .collect();
            shown.part = Gnomad::Asking;
            lookups.shown = Some(shown);
            return Task::perform(
                async move {
                    missing
                        .into_iter()
                        .map(|id| {
                            let answer = gnomad::ask(&id, dataset);
                            (id, answer)
                        })
                        .collect()
                },
                move |answers| Message::Gnomad(Gnomading::Answered(number, dataset, answers)),
            );
        }
        lookups.shown = Some(shown);
        Task::none()
    }

    fn gnomad_answered(&mut self, number: u64, dataset: &'static str, answers: Answers) {
        let lookups = &mut self.gnomad;
        let mut failure = None;
        for (id, answer) in answers {
            match answer {
                Ok(found) => {
                    lookups.answers.insert((dataset, id), found);
                }
                Err(reason) => {
                    failure.get_or_insert(reason);
                }
            }
        }
        if number != lookups.asked {
            return;
        }
        let Some(shown) = lookups.shown.take() else {
            return;
        };
        let part = match failure {
            Some(reason) => Gnomad::Failed(reason),
            None => lookups
                .found(&shown)
                .unwrap_or_else(|| Gnomad::Failed("gnomAD did not answer.".to_owned())),
        };
        lookups.shown = Some(Shown { part, ..shown });
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::Region;
    use genie_core::variant::{Annotations, Variant};
    use genie_formats::fai::FaiRecord;
    use genie_formats::gnomad::{Count, Frequencies};
    use iced::Point;
    use iced_test::simulator;

    use super::Gnomading;
    use crate::app::{Genie, Message};
    use crate::card::{Card, Changes, Detail};
    use crate::reference::Reference;

    /// A genie with a reference whose chromosome 1 has a length, as hg38 for 248,956,422.
    fn genie(length: u64) -> Genie {
        let record = FaiRecord {
            name: "chr1".to_owned(),
            length,
            offset: 6,
            line_bases: 60,
            line_width: 61,
        };
        let reference = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), vec![record]).unwrap());
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference)));
        genie
    }

    /// The open card of a variant at chr1:100 with the reference allele A.
    fn pin(genie: &mut Genie, alternates: &[&str]) {
        let variant = Variant {
            id: "rs1".to_owned(),
            span: Region::between(99, 100),
            reference: "A".to_owned(),
            alternates: alternates
                .iter()
                .map(|allele| (*allele).to_owned())
                .collect(),
            quality: None,
            filter: "PASS".to_owned(),
            info: String::new(),
            annotations: Annotations::default(),
            structure: None,
            calls: Vec::new(),
        };
        genie.pinned = Some(Card {
            track: 0,
            contig: "chr1".to_owned(),
            detail: Detail::Variant {
                variant,
                sample: None,
                groups: Vec::new(),
            },
            at: Point::new(100.0, 100.0),
            samples: Vec::new(),
            fields: Vec::new(),
            effects: Changes::default(),
        });
        let _ = genie.ask_gnomad();
    }

    fn frequencies() -> Frequencies {
        Frequencies {
            all: Count {
                alternate: 120,
                total: 1_000,
            },
            homozygotes: 5,
            groups: vec![(
                "African/African American",
                Count {
                    alternate: 50,
                    total: 200,
                },
            )],
        }
    }

    fn answer(genie: &mut Genie, answers: Vec<(&str, Result<Option<Frequencies>, String>)>) {
        let number = genie.gnomad.asked;
        let answers = answers
            .into_iter()
            .map(|(id, answer)| (id.to_owned(), answer))
            .collect();
        let _ = genie.update(Message::Gnomad(Gnomading::Answered(
            number,
            "gnomad_r4",
            answers,
        )));
    }

    fn shows(genie: &Genie, words: &str) -> bool {
        simulator(genie.view()).find(words).is_ok()
    }

    #[test]
    fn an_open_variant_card_asks_gnomad_once_the_setting_is_on() {
        let mut genie = genie(super::GRCH38);
        pin(&mut genie, &["G"]);
        assert_eq!(
            genie.gnomad.asked, 0,
            "nothing goes out while the setting is off"
        );
        let mut ui = simulator(genie.view());
        ui.click("Ask gnomAD").unwrap();
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(messages.contains(&Message::Gnomad(Gnomading::TurnOn)));
        let _ = genie.update(Message::Gnomad(Gnomading::TurnOn));
        assert!(genie.settings.gnomad);
        assert!(shows(&genie, "gnomAD: asking…"));
        answer(&mut genie, vec![("1-100-A-G", Ok(Some(frequencies())))]);
        let line = "gnomAD v4.1: 12.0 % · 120 of 1,000 alleles · 5 homozygotes";
        assert!(shows(&genie, line));
        assert!(shows(&genie, "African/African American"));
        assert!(shows(&genie, "25.0 % · 50 of 200"));
        let asked = genie.gnomad.asked;
        pin(&mut genie, &["G"]);
        assert_eq!(genie.gnomad.asked, asked, "genie keeps the answer");
        assert!(shows(&genie, line));
    }

    #[test]
    fn alleles_without_an_answer_and_failures_say_so() {
        let mut genie = genie(super::GRCH38);
        genie.settings.gnomad = true;
        pin(&mut genie, &["G", "T"]);
        answer(
            &mut genie,
            vec![
                ("1-100-A-G", Ok(Some(frequencies()))),
                ("1-100-A-T", Ok(None)),
            ],
        );
        let line = "gnomAD v4.1 G: 12.0 % · 120 of 1,000 alleles · 5 homozygotes";
        assert!(shows(&genie, line));
        assert!(shows(&genie, "gnomAD v4.1 T: not in gnomAD"));
        assert!(
            !shows(&genie, "African/African American"),
            "groups only for one allele"
        );
        pin(&mut genie, &["C"]);
        let busy = "The server of gnomad.broadinstitute.org is busy (429 Too Many Requests).";
        answer(&mut genie, vec![("1-100-A-C", Err(busy.to_owned()))]);
        assert!(shows(&genie, busy));
    }

    #[test]
    fn variants_of_other_genomes_have_no_gnomad_part() {
        let mut genie = genie(1_000);
        genie.settings.gnomad = true;
        pin(&mut genie, &["G"]);
        assert!(genie.gnomad.shown.is_none());
        assert_eq!(genie.gnomad.asked, 0);
        let mut human = self::genie(super::GRCH37);
        human.settings.gnomad = true;
        pin(&mut human, &["<DEL>"]);
        assert!(human.gnomad.shown.is_none(), "symbolic alleles");
        pin(&mut human, &["G"]);
        assert!(shows(&human, "gnomAD: asking…"));
    }
}
