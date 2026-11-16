//! Lifts: chain files that move the data of a track, or the regions of the region list, from the
//! places of another assembly to the places of the assembly in view.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

use genie_core::{Assembly, Mark, Region};
use genie_formats::chain::Chains;
use genie_formats::gzip;
use iced::Task;

use super::{Genie, Message};
use crate::location::Location;
use crate::track::{self, Lift};

#[derive(Debug, Clone, PartialEq)]
pub enum Lifting {
    /// Asks for a chain file for a track, or for the region list without a track.
    Pick(Option<u64>),
    Picked(Option<u64>, Option<PathBuf>),
    /// Reads a chain file for a track, or for the region list without a track.
    Open(Option<u64>, Location),
    Read(Option<u64>, Location, Result<Arc<Chains>, String>),
    /// Shows the data of a track without its lift again.
    Stop(u64),
}

/// Reads a chain file, plain or compressed.
pub fn read(location: &Location) -> Result<Arc<Chains>, String> {
    let name = location.name();
    let file = BufReader::new(location.open()?);
    let reader: Box<dyn BufRead> = if track::is_compressed(&name) {
        Box::new(BufReader::new(gzip::Reader::new(file)))
    } else {
        Box::new(file)
    };
    Chains::read(reader)
        .map(Arc::new)
        .map_err(|error| format!("The chain file {name} is not valid. {error}"))
}

async fn pick() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Open a chain file, as hg19ToHg38.over.chain.gz")
        .add_filter("Chain files", &["chain", "gz"])
        .pick_file()
        .await
        .map(|handle| handle.path().to_owned())
}

/// The regions of a list at their places on the assembly in view, from their places on the
/// assembly of the chains. A region that the chains do not move is not in the result.
fn lifted_marks(lift: &Lift, assembly: &Assembly, marks: &[Mark]) -> Vec<(usize, Region, String)> {
    marks
        .iter()
        .filter_map(|mark| {
            let name = &assembly.contigs()[mark.contig].name;
            let (contig, span, _) = lift.place(name, mark.span)?;
            Some((assembly.find(contig)?, span, mark.name.clone()))
        })
        .collect()
}

impl Genie {
    pub(super) fn lift(&mut self, lifting: Lifting) -> Task<Message> {
        match lifting {
            Lifting::Pick(target) => {
                self.close_menus();
                Task::perform(pick(), move |path| {
                    Message::Lift(Lifting::Picked(target, path))
                })
            }
            Lifting::Picked(target, Some(path)) => {
                self.lift(Lifting::Open(target, Location::File(path)))
            }
            Lifting::Picked(_, None) => Task::none(),
            Lifting::Open(target, location) => {
                self.busy += 1;
                Task::perform(
                    async move {
                        let result = read(&location);
                        (location, result)
                    },
                    move |(location, result)| {
                        Message::Lift(Lifting::Read(target, location, result))
                    },
                )
            }
            Lifting::Read(target, location, result) => {
                self.busy = self.busy.saturating_sub(1);
                match (result, target) {
                    (Ok(chains), Some(id)) => self.set_lift(id, Some((location, chains))),
                    (Ok(chains), None) => self.lift_regions(location, chains),
                    (Err(error), target) => {
                        self.on_session(|session| {
                            let track = session
                                .tracks
                                .iter_mut()
                                .find(|track| Some(track.id) == target);
                            if let Some(track) = track {
                                track.lifting = None;
                            }
                        });
                        self.fail(error);
                        Task::none()
                    }
                }
            }
            Lifting::Stop(id) => {
                self.close_menus();
                self.set_lift(id, None)
            }
        }
    }

    /// Moves the data of a track to the assembly in view with a chain file, or shows the data
    /// as it is without one, then loads the data and the names of the track again.
    fn set_lift(&mut self, id: u64, lift: Option<(Location, Arc<Chains>)>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let assembly = &session.reference.assembly;
        let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) else {
            return Task::none();
        };
        let lift = lift.map(|(location, chains)| {
            Arc::new(Lift::new(location, chains, track.source.contigs()))
        });
        track.set_lift(lift, assembly);
        self.forget(id);
        Task::batch([self.fetch_features(), self.find_names(id)])
    }

    /// Moves the regions of the region list from their places on another assembly to their
    /// places on the assembly in view.
    fn lift_regions(&mut self, location: Location, chains: Arc<Chains>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let assembly = &session.reference.assembly;
        let names: Vec<String> = assembly
            .contigs()
            .iter()
            .map(|contig| contig.name.clone())
            .collect();
        let lift = Lift::new(location, chains, &names);
        let marks = session.marks.all().to_vec();
        let lifted = lifted_marks(&lift, assembly, &marks);
        if lifted.is_empty() {
            let text = format!(
                "The chain file {} moves none of the regions. The list did not change.",
                lift.location.name()
            );
            self.warn(text);
            return Task::none();
        }
        for mark in &marks {
            session.marks.remove(mark.id);
        }
        for (contig, span, name) in &lifted {
            session.marks.add(*contig, *span, Some(name.clone()));
        }
        let regions = |count: usize| match count {
            1 => "1 region".to_owned(),
            _ => format!("{count} regions"),
        };
        let text = match marks.len() - lifted.len() {
            0 => format!("Lifted {}.", regions(lifted.len())),
            lost => format!(
                "Lifted {} of {}. {lost} had no place on this assembly.",
                lifted.len(),
                regions(marks.len())
            ),
        };
        self.inform(text)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::{Assembly, Contig, Marks, Region};
    use genie_formats::chain::Chains;

    use super::lifted_marks;
    use crate::location::Location;
    use crate::track::Lift;

    #[test]
    fn regions_move_to_the_assembly_in_view() {
        let chains = "chain 1000 chr1 5000 + 0 3000 chr1 6000 + 0 3100 1\n1000 0 100\n2000\n";
        let chains = Arc::new(Chains::read(Cursor::new(chains)).unwrap());
        let assembly = Assembly::new(
            ["chr1", "chr2"]
                .map(|name| Contig {
                    name: name.to_owned(),
                    length: 6_000,
                })
                .into(),
        );
        let names = ["chr1".to_owned(), "chr2".to_owned()];
        let lift = Lift::new(Location::File(PathBuf::from("a.chain")), chains, &names);
        let mut marks = Marks::default();
        marks.add(0, Region::between(1_500, 1_600), Some("Exon".to_owned()));
        marks.add(1, Region::between(0, 10), Some("Elsewhere".to_owned()));
        let lifted = lifted_marks(&lift, &assembly, marks.all());
        assert_eq!(
            lifted,
            [(0, Region::between(1_600, 1_700), "Exon".to_owned())]
        );
    }
}
