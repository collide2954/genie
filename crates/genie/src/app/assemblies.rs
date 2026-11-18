//! NCBI assemblies through UCSC `GenArk`: a dialog finds an assembly by its species, its common
//! name or its accession in the `GenArk` list, then genie opens the `GenArk` hub of the
//! assembly and its genome.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use genie_formats::genark::{self, Assembly};
use genie_formats::http::Url;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, center, column, container, opaque, operation, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::hub;
use crate::icon::{self, Icon};
use crate::location::Location;
use crate::{font, style};

use super::{Genie, Message, Popup};

const SEARCH: &str = "assembly-search";
const WIDTH: f32 = 560.0;
/// The most assemblies that the dialog shows.
const MOST: usize = 100;
const LIST_HEIGHT: f32 = 320.0;
/// The age of a saved `GenArk` list after which genie reads it again.
const FRESH: Duration = Duration::from_hours(7 * 24);
/// The folder in the cache that keeps the `GenArk` list.
const FOLDER: &str = "genark";
const FILE: &str = "assemblies.txt";
const READING: &str = "Reading the list of UCSC GenArk…";
const HINT: &str =
    "Type a species, a common name or an accession, for example Gallus gallus or GCF_016699485.2.";
const NONE: &str = "No assembly of UCSC GenArk has these words.";

#[derive(Debug, Clone, PartialEq)]
pub enum Assembling {
    /// Shows the dialog, and reads the `GenArk` list if genie does not have it.
    Open,
    Query(String),
    Read(Result<Arc<[Assembly]>, String>),
    /// Opens the hub of an accession, then its genome.
    Pick(String),
}

/// The state of the dialog and the `GenArk` list.
#[derive(Debug, Default)]
pub(super) struct Assemblies {
    list: Option<Arc<[Assembly]>>,
    query: String,
    /// The assemblies of the list that match the query. genie finds them when the query or the
    /// list changes, not at each draw of the dialog.
    found: Vec<Assembly>,
    reading: bool,
    error: Option<String>,
    /// The hub of the assembly that somebody chose. When it arrives, genie opens its genome.
    pub(super) wanted: Option<Location>,
}

impl Assemblies {
    fn find(&mut self) {
        self.found = self.list.as_deref().map_or_else(Vec::new, |list| {
            let found = genark::search(list, &self.query, MOST);
            found.into_iter().cloned().collect()
        });
    }
}

/// The location of the `GenArk` hub of an accession.
pub(super) fn hub_location(accession: &str) -> Option<Location> {
    let address = genark::hub(accession)?;
    Url::parse(&address).ok().map(Location::Url)
}

/// The `GenArk` list from the cache when it is fresh, else from UCSC. When UCSC does not
/// answer, an old list in the cache will do.
pub(super) fn read_list(
    cache: Option<&Path>,
    source: &Location,
    now: SystemTime,
) -> Result<Arc<[Assembly]>, String> {
    let saved: Option<PathBuf> = cache.map(|cache| cache.join(FOLDER).join(FILE));
    let old = saved
        .as_ref()
        .and_then(|path| Some((fs::read_to_string(path).ok()?, fs::metadata(path).ok()?)));
    let fresh = |metadata: &fs::Metadata| {
        metadata
            .modified()
            .is_ok_and(|time| now.duration_since(time).map_or(true, |age| age < FRESH))
    };
    if let Some((text, metadata)) = &old
        && fresh(metadata)
    {
        return Ok(genark::list(text).into());
    }
    match hub::read_text(source) {
        Ok(text) => {
            if let Some(path) = &saved {
                let _ = path.parent().map(fs::create_dir_all);
                let _ = fs::write(path, &text);
            }
            Ok(genark::list(&text).into())
        }
        Err(error) => match old {
            Some((text, _)) => Ok(genark::list(&text).into()),
            None => Err(format!(
                "genie cannot read the list of UCSC GenArk. {error}"
            )),
        },
    }
}

impl Genie {
    pub(super) fn assembling(&mut self, assembling: Assembling) -> Task<Message> {
        let state = &mut self.assemblies;
        match assembling {
            Assembling::Open => {
                self.close_menus();
                self.popup = Some(Popup::Assemblies);
                let focus = operation::focus(SEARCH);
                let state = &mut self.assemblies;
                if state.list.is_some() || state.reading {
                    return focus;
                }
                state.reading = true;
                state.error = None;
                let cache = self.cache.clone();
                let read = Task::perform(
                    async move {
                        let source = Url::parse(genark::LIST)
                            .map(Location::Url)
                            .map_err(|error| error.to_string())?;
                        read_list(cache.as_deref(), &source, SystemTime::now())
                    },
                    |result| Message::Assemblies(Assembling::Read(result)),
                );
                return Task::batch([focus, read]);
            }
            Assembling::Query(query) => {
                state.query = query;
                state.find();
            }
            Assembling::Read(result) => {
                state.reading = false;
                match result {
                    Ok(list) => {
                        state.list = Some(list);
                        state.find();
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            Assembling::Pick(accession) => {
                let Some(location) = hub_location(&accession) else {
                    return Task::none();
                };
                self.close_menus();
                self.assemblies.wanted = Some(location.clone());
                return self.open_hub(location, true);
            }
        }
        Task::none()
    }

    /// The rows of the dialog: an accession that genie can open without the list, then the
    /// assemblies of the list that match the words.
    fn assembly_rows(&self) -> Vec<Element<'_, Message>> {
        let state = &self.assemblies;
        let found = &state.found;
        let mut rows = Vec::new();
        if let Some(accession) = genark::accession(&state.query)
            && !found.iter().any(|assembly| assembly.accession == accession)
        {
            let words = format!("Open {accession} from UCSC GenArk");
            rows.push(pick(text(words).size(12).into(), accession));
        }
        rows.extend(found.iter().map(|assembly| {
            let title = if assembly.common.is_empty() {
                assembly.species.clone()
            } else {
                format!("{} ({})", assembly.species, assembly.common)
            };
            let mut about = if assembly.name.is_empty() {
                assembly.accession.clone()
            } else {
                format!("{} · {}", assembly.name, assembly.accession)
            };
            if assembly.legacy {
                about.push_str(" · outdated");
            }
            let content = column![
                text(title).size(12),
                text(about).size(11).style(style::muted)
            ];
            pick(content.into(), assembly.accession.clone())
        }));
        rows
    }

    pub(super) fn assembly_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Assemblies) {
            return None;
        }
        let state = &self.assemblies;
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let heading = row![
            text("NCBI assemblies").font(font::UI_STRONG).size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let search = text_input("Species, common name or accession", &state.query)
            .id(SEARCH)
            .size(12)
            .padding([3, 8])
            .style(style::locus)
            .on_input(|query| Message::Assemblies(Assembling::Query(query)));
        let rows = self.assembly_rows();
        let note = if let Some(error) = &state.error {
            Some(error.as_str())
        } else if state.reading {
            Some(READING)
        } else if state.query.trim().is_empty() {
            Some(HINT)
        } else if rows.is_empty() {
            Some(NONE)
        } else {
            None
        };
        let mut body = column![heading, search].spacing(10);
        if let Some(note) = note {
            body = body.push(text(note).size(11).style(style::muted));
        }
        if !rows.is_empty() {
            let list = scrollable(Column::with_children(rows).spacing(1))
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0),
                ))
                .style(style::scrollbar)
                .height(Length::Shrink);
            body = body.push(container(list).max_height(LIST_HEIGHT));
        }
        let card = container(body)
            .id("assembly-dialog")
            .width(WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

/// A row of the dialog that opens the hub of an accession.
fn pick(content: Element<'_, Message>, accession: String) -> Element<'_, Message> {
    button(content)
        .width(Length::Fill)
        .padding([4, 8])
        .style(style::ghost)
        .on_press(Message::Assemblies(Assembling::Pick(accession)))
        .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime};

    use genie_formats::genark::{self, Assembly};
    use iced_test::simulator;

    use super::{Assembling, read_list};
    use crate::app::{Genie, Message};
    use crate::hub::{self, Wanted};
    use crate::location::Location;
    use crate::scratch::Scratch;

    const LIST: &str = "# accession\tassembly\tscientific name\tcommon name\ttaxonId\tGenArk clade\n\
        GCA_000002315.3\tGallus_gallus-4.0\tGallus gallus\tchicken (2011)\t9031\tbirds(L)\n\
        GCF_016699485.2\tbGalGal1.mat.broiler.GRCg7b\tGallus gallus\tchicken\t9031\tbirds\n\
        GCF_000001405.40\tGRCh38.p14\tHomo sapiens\thuman\t9606\tprimates\n";

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    fn list() -> Arc<[Assembly]> {
        genark::list(LIST).into()
    }

    /// The sizes of the real files of these checks, for one byte each.
    #[test]
    #[ignore = "fetches real data from the web"]
    fn real_sizes() {
        let chicken = genark::hub("GCF_016699485.2").unwrap();
        for address in [genark::LIST, chicken.as_str()] {
            let url = genie_formats::http::Url::parse(address).unwrap();
            let size = genie_formats::http::fetch(&url, 0, 1).unwrap().size;
            println!("{address}: {size} bytes");
        }
    }

    /// The start of the real list of UCSC (128 KB of its 6 MB): its first lines, its count, and
    /// what searches find in it.
    #[test]
    #[ignore = "fetches real data from the web"]
    fn real_genark_list() {
        let url = genie_formats::http::Url::parse(genark::LIST).unwrap();
        let start = genie_formats::http::fetch(&url, 0, 128 << 10).unwrap();
        let text = String::from_utf8_lossy(&start.bytes);
        let whole = text.rfind('\n').map_or(&text[..], |end| &text[..end]);
        for line in whole.lines().take(4) {
            println!("{line}");
        }
        let all = genark::list(whole);
        println!(
            "{} assemblies in {} lines",
            all.len(),
            whole.lines().count()
        );
        for query in ["chicken", "Gallus", "GCA_", "Homo sapiens", "fish"] {
            let found: Vec<String> = genark::search(&all, query, 4)
                .into_iter()
                .map(|assembly| {
                    let (name, species) = (&assembly.name, &assembly.species);
                    format!(
                        "{} {name} {species} ({})",
                        assembly.accession, assembly.common
                    )
                })
                .collect();
            println!("{query}: {found:?}");
        }
        assert!(all.len() > 100, "{}", all.len());
        assert!(all.iter().all(|assembly| !assembly.species.is_empty()));
    }

    /// The real hub of the chicken assembly: its genome and its sequence, then its tracks once
    /// the sequence is open.
    #[test]
    #[ignore = "fetches real data from the web"]
    fn real_genark_hub() {
        let location = super::hub_location("GCF_016699485.2").unwrap();
        let found = hub::load(&location, Wanted::default()).unwrap();
        println!("{} · {} genomes", found.short, found.genomes.len());
        let genome = &found.genomes[0];
        println!(
            "{} · {} · {:?} · {:?}",
            genome.name, genome.organism, genome.sequence, genome.position
        );
        let (sequence, sizes) = (
            genome.sequence.clone().unwrap(),
            genome.sizes.clone().unwrap(),
        );
        let reference =
            crate::reference::open_sized(sequence.clone(), &sizes, None, &mut |_| {}).unwrap();
        let contigs = reference.assembly.contigs();
        let first: Vec<String> = contigs
            .iter()
            .take(4)
            .map(|contig| format!("{} {}", contig.name, contig.length))
            .collect();
        println!("{} contigs: {first:?}", contigs.len());
        let bases = reference
            .fetch(0, genie_core::Region::between(1_000_000, 1_000_040))
            .unwrap();
        println!("{}", String::from_utf8_lossy(&bases));
        let wanted = Wanted {
            genome: None,
            reference: Some(&sequence),
        };
        let open = hub::load(&location, wanted).unwrap();
        let tracks: Vec<String> = open
            .tracks
            .iter()
            .take(8)
            .map(|track| format!("{} ({})", track.short, track.kind))
            .collect();
        println!(
            "chosen {:?}, {} tracks: {tracks:?}",
            open.chosen,
            open.tracks.len()
        );
        assert_eq!(open.chosen, Some(0));
    }

    #[test]
    fn the_genome_menu_finds_ncbi_assemblies_by_species() {
        let mut genie = Genie::default();
        let mut ui = simulator(genie.view());
        ui.click("Other NCBI assemblies…").unwrap();
        let open = Message::Assemblies(Assembling::Open);
        assert!(ui.into_messages().any(|message| message == open));
        let _ = genie.update(Message::GenomeMenu);
        assert!(shows(&genie, "NCBI assembly…"));
        let _ = genie.update(Message::Assemblies(Assembling::Open));
        assert!(shows(&genie, "NCBI assemblies"));
        assert!(shows(&genie, "Reading the list of UCSC GenArk…"));
        let _ = genie.update(Message::Assemblies(Assembling::Query("chicken".to_owned())));
        let _ = genie.update(Message::Assemblies(Assembling::Read(Ok(list()))));
        assert!(shows(&genie, "Gallus gallus (chicken)"));
        assert!(shows(
            &genie,
            "bGalGal1.mat.broiler.GRCg7b · GCF_016699485.2"
        ));
        assert!(shows(
            &genie,
            "Gallus_gallus-4.0 · GCA_000002315.3 · outdated"
        ));
        let found = |genie: &Genie| -> Vec<String> {
            let found = &genie.assemblies.found;
            found.iter().map(|one| one.accession.clone()).collect()
        };
        assert_eq!(found(&genie), ["GCF_016699485.2", "GCA_000002315.3"]);
        assert!(!shows(&genie, "Homo sapiens (human)"));
        let _ = genie.update(Message::Assemblies(Assembling::Query("human".to_owned())));
        assert_eq!(found(&genie), ["GCF_000001405.40"]);
        let busy = genie.busy;
        let pick = Assembling::Pick("GCF_016699485.2".to_owned());
        let _ = genie.update(Message::Assemblies(pick));
        assert_eq!(genie.busy, busy + 1);
        assert!(!shows(&genie, "NCBI assemblies"));
        let Some(Location::Url(url)) = &genie.assemblies.wanted else {
            panic!("no wanted hub");
        };
        assert_eq!(
            url.to_string(),
            "https://hgdownload.soe.ucsc.edu/hubs/GCF/016/699/485/GCF_016699485.2/hub.txt"
        );
    }

    #[test]
    fn accessions_open_without_the_list() {
        let mut genie = Genie::default();
        let _ = genie.update(Message::Assemblies(Assembling::Open));
        let error = Assembling::Read(Err("UCSC does not answer.".to_owned()));
        let _ = genie.update(Message::Assemblies(error));
        assert!(shows(&genie, "UCSC does not answer."));
        let query = Assembling::Query("gca_009914755.4".to_owned());
        let _ = genie.update(Message::Assemblies(query));
        assert!(shows(&genie, "Open GCA_009914755.4 from UCSC GenArk"));
    }

    #[test]
    fn address_fields_take_accessions() {
        use crate::app::{Address, hubs::Hubbing};

        for ask in [
            Message::Hub(Hubbing::Ask),
            Message::Command(crate::shortcuts::Command::OpenUrl),
        ] {
            let mut genie = Genie::default();
            let _ = genie.update(ask);
            let typed = Address::Edit(" GCF_000001405.40 ".to_owned());
            let _ = genie.update(Message::Address(typed));
            let _ = genie.update(Message::Address(Address::Submit));
            let Some(Location::Url(url)) = &genie.assemblies.wanted else {
                panic!("no wanted hub");
            };
            assert!(url.to_string().ends_with("/GCF_000001405.40/hub.txt"));
            assert_eq!(genie.busy, 1);
        }
    }

    #[test]
    fn a_chosen_assembly_opens_its_genome_when_its_hub_arrives() {
        let text = "hub GCF_1\nshortLabel Chicken\nuseOneFile on\nemail a@b.c\n\n\
            genome GCF_016699485.2\ntwoBitPath GCF_016699485.2.2bit\norganism Gallus gallus\n";
        let scratch = Scratch::new("app-assembly");
        let location = Location::File(scratch.file("hub.txt", text));
        let mut genie = Genie::default();
        genie.assemblies.wanted = Some(location.clone());
        let loaded = hub::load(&location, Wanted::default()).map(Arc::new);
        let message = crate::app::hubs::Hubbing::Loaded(location.clone(), true, loaded);
        let _ = genie.update(Message::Hub(message));
        assert!(shows(&genie, "Opening GCF_016699485.2.2bit…"));
        assert_eq!(genie.hubs_next, [(location, true)]);
        assert_eq!(genie.assemblies.wanted, None);
    }

    #[test]
    fn the_list_comes_from_the_cache_while_it_is_fresh() {
        let scratch = Scratch::new("app-assembly-cache");
        let cache = scratch.file("marker", "").parent().unwrap().to_owned();
        let source = Location::File(scratch.file("remote.txt", LIST));
        let now = SystemTime::now();
        let first = read_list(Some(&cache), &source, now).unwrap();
        assert_eq!(first.len(), 3);
        let saved: PathBuf = cache.join("genark").join("assemblies.txt");
        assert!(saved.is_file());
        std::fs::write(
            &saved,
            "GCF_000001405.40\tGRCh38.p14\tHomo sapiens\thuman\n",
        )
        .unwrap();
        assert_eq!(read_list(Some(&cache), &source, now).unwrap().len(), 1);
        let later = now + Duration::from_hours(8 * 24);
        assert_eq!(read_list(Some(&cache), &source, later).unwrap().len(), 3);
        let gone = Location::File(cache.join("absent.txt"));
        assert_eq!(read_list(Some(&cache), &gone, later).unwrap().len(), 3);
        assert!(read_list(None, &gone, now).is_err());
    }
}
