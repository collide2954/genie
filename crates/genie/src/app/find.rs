use std::sync::Arc;

use genie_core::search::{
    self, Found, Hit, MOST_MISMATCHES, Pattern, PatternError, Reach, Scanned,
};
use genie_core::{Locus, Region, Span, Strand, Target, Visit, codon, grouped};
use genie_render::matches;
use iced::widget::{button, container, operation, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length, Task};

use crate::card::{Card, Detail};
use crate::chrome;
use crate::icon::{self, Icon};
use crate::scene::{self, Pointer};
use crate::{font, style};

use super::{Genie, Message, Session};

pub(super) const FIELD: &str = "find";
pub(super) const LANE: u64 = u64::MAX;
pub(super) const BAR_HEIGHT: f32 = 28.0;
const LIMIT: u64 = 10_000_000;
const MIN_MARGIN: u64 = 10_000;
const MARGIN: u64 = 2_000_000;
const HITS: usize = 100_000;
const CHUNK: u64 = 2_000_000;
const BUDGET: u64 = 20_000_000;
const FIELD_WIDTH: f32 = 240.0;
const BUTTON: f32 = 20.0;
const TOO_WIDE: &str = "Zoom in to 10 Mb or less to find a sequence.";

#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    Edit(String),
    Mismatches(u8),
    Submit,
    Step(bool),
    Searched(u64, usize, Result<Arc<Searched>, String>),
    Scanned(u64, Result<Swept, String>),
    Hover(usize, Option<Pointer>),
    Click(usize, Pointer),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Searched {
    pub contig: usize,
    pub region: Region,
    pub bases: Arc<[u8]>,
    pub found: Found,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Swept {
    pub contig: usize,
    pub forward: bool,
    pub from: u64,
    pub scanned: Scanned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Resume {
    visit: Visit,
    forward: bool,
    contig: usize,
    from: u64,
}

enum Step {
    Show(usize, Region),
    Scan(Pattern, usize, u64),
}

#[derive(Debug, Default)]
pub(super) struct Finder {
    text: String,
    mismatches: u8,
    pattern: Option<Pattern>,
    error: Option<String>,
    generation: u64,
    panels: Vec<Option<(u64, Arc<Searched>)>>,
    wanted: Vec<Option<(usize, Region)>>,
    current: Option<(Visit, usize, Region)>,
    resume: Option<Resume>,
    scanning: bool,
    note: Option<String>,
    failed: Option<String>,
}

impl Finder {
    fn results(&self, panel: usize, contig: usize) -> Option<&Searched> {
        self.panels
            .get(panel)?
            .as_ref()
            .filter(|(generation, searched)| {
                *generation == self.generation && searched.contig == contig
            })
            .map(|(_, searched)| searched.as_ref())
    }

    fn parse(&mut self) {
        let parsed = Pattern::parse(&self.text)
            .and_then(|pattern| pattern.check(self.mismatches).map(|()| pattern));
        (self.pattern, self.error) = match parsed {
            Ok(pattern) => (Some(pattern), None),
            Err(PatternError::Empty) => (None, None),
            Err(error) => (None, Some(error.to_string())),
        };
    }

    fn restart(&mut self) {
        self.generation += 1;
        self.wanted.iter_mut().for_each(|wanted| *wanted = None);
        self.current = None;
        self.resume = None;
        self.scanning = false;
        self.note = None;
        self.failed = None;
        self.parse();
    }

    fn forget(&mut self) {
        self.restart();
        self.panels.clear();
        self.wanted.clear();
    }
}

pub fn search_bases(
    pattern: &Pattern,
    mismatches: u8,
    contig: usize,
    region: Region,
    bases: Arc<[u8]>,
) -> Searched {
    let found = search::find(pattern, &bases, region.start(), mismatches, HITS);
    Searched {
        contig,
        region,
        bases,
        found,
    }
}

fn covers(outer: Region, inner: Region) -> bool {
    outer.start() <= inner.start() && inner.end() <= outer.end()
}

/// The bases of a match as read on its strand, with the wrong bases in lower case.
pub fn match_bases(bases: &[u8], hit: &Hit) -> String {
    let mut letters: Vec<u8> = bases.iter().map(u8::to_ascii_uppercase).collect();
    for offset in &hit.mismatches {
        if let Some(letter) = letters.get_mut(*offset as usize) {
            *letter = letter.to_ascii_lowercase();
        }
    }
    if hit.strand == Strand::Reverse {
        letters.reverse();
        for letter in &mut letters {
            *letter = codon::complement(*letter);
        }
    }
    String::from_utf8_lossy(&letters).into_owned()
}

fn small<'a>(icon: Icon, message: Message) -> Element<'a, Message> {
    button(icon::view(icon, 12.0))
        .width(BUTTON)
        .height(BUTTON)
        .padding(0)
        .style(style::ghost)
        .on_press(message)
        .into()
}

impl Genie {
    pub(super) fn find(&mut self, finding: Finding) -> Task<Message> {
        match finding {
            Finding::Edit(text) => {
                if let Some(finder) = &mut self.finder {
                    finder.text = text;
                }
                return self.restart_finder();
            }
            Finding::Mismatches(count) => {
                if let Some(finder) = &mut self.finder {
                    finder.mismatches = count.min(MOST_MISMATCHES);
                }
                return self.restart_finder();
            }
            Finding::Submit => return self.step_match(!self.modifiers.shift()),
            Finding::Step(forward) => return self.step_match(forward),
            Finding::Searched(generation, panel, result) => {
                self.accept_search(generation, panel, result);
            }
            Finding::Scanned(generation, result) => return self.swept(generation, result),
            Finding::Hover(panel, pointer) => {
                self.hover = pointer.and_then(|pointer| self.match_card(panel, pointer));
            }
            Finding::Click(panel, pointer) => self.pinned = self.match_card(panel, pointer),
            Finding::Close => self.close_finder(),
        }
        Task::none()
    }

    pub(super) fn open_finder(&mut self) -> Task<Message> {
        if self.session.is_none() {
            return Task::none();
        }
        self.close_menus();
        if self.finder.is_none() {
            self.finder = Some(Finder::default());
        }
        Task::batch([operation::focus(FIELD), operation::select_all(FIELD)])
    }

    pub(super) fn close_finder(&mut self) {
        self.finder = None;
        self.drop_match_cards();
    }

    pub(super) fn forget_matches(&mut self) {
        if let Some(finder) = &mut self.finder {
            finder.forget();
        }
        self.drop_match_cards();
    }

    fn drop_match_cards(&mut self) {
        if self.hover.as_ref().is_some_and(|card| card.track == LANE) {
            self.hover = None;
        }
        if self.pinned.as_ref().is_some_and(|card| card.track == LANE) {
            self.pinned = None;
        }
    }

    fn restart_finder(&mut self) -> Task<Message> {
        if let Some(finder) = &mut self.finder {
            finder.restart();
        }
        self.drop_match_cards();
        self.search_view()
    }

    pub(super) fn search_view(&mut self) -> Task<Message> {
        let (Some(finder), Some(session)) = (&mut self.finder, &self.session) else {
            return Task::none();
        };
        let Some(pattern) = finder.pattern.clone() else {
            return Task::none();
        };
        let count = session.count();
        finder.panels.resize(count, None);
        finder.wanted.resize(count, None);
        let mut tasks = Vec::new();
        for panel in 0..count {
            let (contig, viewport) = session.frame(panel);
            let visible = viewport.visible();
            if visible.len() > LIMIT {
                continue;
            }
            let fresh = finder
                .results(panel, contig)
                .is_some_and(|searched| covers(searched.region, visible));
            let flying = finder.wanted[panel]
                .is_some_and(|(at, region)| at == contig && covers(region, visible));
            if fresh || flying {
                continue;
            }
            let cached = finder.panels[panel].as_ref().and_then(|(_, searched)| {
                (searched.contig == contig && covers(searched.region, visible))
                    .then(|| (searched.region, Arc::clone(&searched.bases)))
            });
            let length = session.reference.assembly.contigs()[contig].length;
            let margin = (visible.len() / 2).clamp(MIN_MARGIN, MARGIN);
            let region = cached
                .as_ref()
                .map_or_else(|| visible.expand(margin, length), |(region, _)| *region);
            finder.wanted[panel] = Some((contig, region));
            let (generation, mismatches) = (finder.generation, finder.mismatches);
            let (reference, pattern) = (Arc::clone(&session.reference), pattern.clone());
            tasks.push(Task::perform(
                async move {
                    let bases: Arc<[u8]> = match cached {
                        Some((_, bases)) => bases,
                        None => reference.fetch(contig, region)?.into(),
                    };
                    let searched = search_bases(&pattern, mismatches, contig, region, bases);
                    Ok::<_, String>(Arc::new(searched))
                },
                move |result| Message::Find(Finding::Searched(generation, panel, result)),
            ));
        }
        Task::batch(tasks)
    }

    fn accept_search(
        &mut self,
        generation: u64,
        panel: usize,
        result: Result<Arc<Searched>, String>,
    ) {
        let Some(finder) = &mut self.finder else {
            return;
        };
        if generation != finder.generation {
            return;
        }
        let Some(Some((contig, region))) = finder.wanted.get(panel).copied() else {
            return;
        };
        match result {
            Ok(searched) if searched.contig == contig && searched.region == region => {
                finder.wanted[panel] = None;
                finder.panels[panel] = Some((generation, searched));
                finder.failed = None;
            }
            Ok(_) => {}
            Err(error) => {
                finder.wanted[panel] = None;
                finder.failed = Some(error);
            }
        }
    }

    fn plan_step(&self, forward: bool) -> Option<Step> {
        let (finder, session) = (self.finder.as_ref()?, self.session.as_ref()?);
        let pattern = finder.pattern.clone()?;
        if finder.scanning {
            return None;
        }
        let visit = session.visit();
        let (contig, viewport) = session.frame(0);
        if let Some(resume) = finder
            .resume
            .filter(|resume| resume.visit == visit && resume.forward == forward)
        {
            return Some(Step::Scan(pattern, resume.contig, resume.from));
        }
        let anchor = match finder.current {
            Some((seen, at, span)) if seen == visit && at == contig => span.start(),
            _ => viewport.center().max(0.0) as u64,
        };
        let Some(searched) = finder
            .results(0, contig)
            .filter(|searched| searched.region.contains(anchor))
        else {
            let from = if forward { anchor + 1 } else { anchor };
            return Some(Step::Scan(pattern, contig, from));
        };
        let hits = &searched.found.hits;
        let next = if forward {
            hits.iter().find(|hit| hit.span.start() > anchor)
        } else {
            hits.iter().rev().find(|hit| hit.span.start() < anchor)
        };
        if let Some(hit) = next {
            return Some(Step::Show(contig, hit.span));
        }
        let tail = pattern.len() as u64 - 1;
        let from = if !forward {
            searched.region.start().min(anchor)
        } else if searched.found.complete {
            searched.region.end().saturating_sub(tail).max(anchor + 1)
        } else {
            hits.last()
                .map_or(anchor + 1, |hit| hit.span.start() + 1)
                .max(anchor + 1)
        };
        Some(Step::Scan(pattern, contig, from))
    }

    fn step_match(&mut self, forward: bool) -> Task<Message> {
        match self.plan_step(forward) {
            Some(Step::Show(contig, span)) => self.show_match(contig, span),
            Some(Step::Scan(pattern, contig, from)) => {
                self.scan_from(&pattern, contig, from, forward)
            }
            None => Task::none(),
        }
    }

    fn scan_from(
        &mut self,
        pattern: &Pattern,
        contig: usize,
        from: u64,
        forward: bool,
    ) -> Task<Message> {
        let (Some(finder), Some(session)) = (&mut self.finder, &self.session) else {
            return Task::none();
        };
        let length = session.reference.assembly.contigs()[contig].length;
        let reach = Reach {
            from,
            forward,
            length,
            budget: BUDGET,
            chunk: CHUNK,
        };
        finder.scanning = true;
        finder.note = None;
        let (generation, mismatches) = (finder.generation, finder.mismatches);
        let (reference, pattern) = (Arc::clone(&session.reference), pattern.clone());
        Task::perform(
            async move {
                search::scan(&pattern, mismatches, reach, |region| {
                    reference.fetch(contig, region)
                })
                .map(|scanned| Swept {
                    contig,
                    forward,
                    from,
                    scanned,
                })
            },
            move |result| Message::Find(Finding::Scanned(generation, result)),
        )
    }

    fn swept(&mut self, generation: u64, result: Result<Swept, String>) -> Task<Message> {
        let (Some(finder), Some(session)) = (&mut self.finder, &self.session) else {
            return Task::none();
        };
        if generation != finder.generation {
            return Task::none();
        }
        finder.scanning = false;
        let swept = match result {
            Ok(swept) => swept,
            Err(error) => {
                finder.failed = Some(error);
                return Task::none();
            }
        };
        if let Some(hit) = &swept.scanned.hit {
            let span = hit.span;
            return self.show_match(swept.contig, span);
        }
        let contigs = session.reference.assembly.contigs();
        let here = &contigs[swept.contig];
        let reached = swept.scanned.reached;
        let push = if swept.forward {
            "Enter"
        } else {
            "Shift+Enter"
        };
        let ended = if swept.forward {
            reached >= here.length
        } else {
            reached == 0
        };
        let (note, contig, from) = if ended {
            let next = if swept.forward {
                (swept.contig + 1) % contigs.len()
            } else {
                (swept.contig + contigs.len() - 1) % contigs.len()
            };
            let other = &contigs[next];
            let text = if swept.forward {
                format!(
                    "No match from {}:{} to the end of {}. Push {push} to go on in {}.",
                    here.name,
                    grouped(swept.from + 1),
                    here.name,
                    other.name
                )
            } else {
                format!(
                    "No match from the start of {} to {}:{}. Push {push} to go on in {}.",
                    here.name,
                    here.name,
                    grouped(swept.from),
                    other.name
                )
            };
            let from = if swept.forward { 0 } else { other.length };
            (text, next, from)
        } else {
            let locus = Locus {
                name: here.name.clone(),
                span: Span::Region(Region::between(swept.from, reached)),
            };
            let text = format!("No match in {locus}. Push {push} again to search on.");
            (text, swept.contig, reached)
        };
        finder.note = Some(note);
        finder.resume = Some(Resume {
            visit: session.visit(),
            forward: swept.forward,
            contig,
            from,
        });
        Task::none()
    }

    fn show_match(&mut self, contig: usize, span: Region) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let target = if span.len() as f64 <= session.destination().span() {
            Span::Position(span.start() + span.len() / 2)
        } else {
            Span::Region(span)
        };
        let task = self.go_to(Target {
            contig,
            span: target,
        });
        if let (Some(finder), Some(session)) = (&mut self.finder, &self.session) {
            finder.current = Some((session.visit(), contig, span));
            finder.resume = None;
            finder.note = None;
        }
        task
    }

    fn match_card(&self, panel: usize, pointer: Pointer) -> Option<Card> {
        let (finder, session) = (self.finder.as_ref()?, self.session.as_ref()?);
        let (contig, viewport) = session.view(panel);
        let searched = finder.results(panel, contig)?;
        let index = matches::hit(viewport, &searched.found.hits, pointer.local.x)?;
        let hit = &searched.found.hits[index];
        let offset = usize::try_from(hit.span.start() - searched.region.start()).ok()?;
        let length = usize::try_from(hit.span.len()).ok()?;
        let bases = searched.bases.get(offset..offset + length)?;
        Some(Card {
            track: LANE,
            contig: session.reference.assembly.contigs()[contig].name.clone(),
            detail: Detail::Match {
                pattern: finder.pattern.as_ref()?.text().to_owned(),
                span: hit.span,
                strand: hit.strand,
                bases: match_bases(bases, hit),
                wrong: hit.mismatches.len(),
            },
            at: pointer.at,
            fields: Vec::new(),
        })
    }

    fn find_status(&self, finder: &Finder) -> (String, bool) {
        if let Some(error) = &finder.error {
            return (error.clone(), true);
        }
        if finder.pattern.is_none() {
            return (String::new(), false);
        }
        if let Some(error) = &finder.failed {
            return (format!("genie cannot read the sequence. {error}"), true);
        }
        if finder.scanning {
            return ("Searching…".to_owned(), false);
        }
        if let Some(note) = &finder.note {
            return (note.clone(), false);
        }
        let Some(session) = &self.session else {
            return (String::new(), false);
        };
        let (contig, viewport) = session.view(0);
        let visible = viewport.visible();
        if visible.len() > LIMIT {
            return (TOO_WIDE.to_owned(), false);
        }
        let Some(searched) = finder
            .results(0, contig)
            .filter(|searched| covers(searched.region, visible))
        else {
            return ("Searching…".to_owned(), false);
        };
        if !searched.found.complete {
            let most = grouped(HITS as u64);
            return (format!("More than {most} matches. Zoom in."), false);
        }
        let shown: Vec<&Hit> = searched
            .found
            .hits
            .iter()
            .filter(|hit| hit.span.overlaps(visible))
            .collect();
        let current = finder
            .current
            .filter(|(seen, at, _)| *seen == session.visit() && *at == contig)
            .and_then(|(_, _, span)| shown.iter().position(|hit| hit.span == span));
        let words = match (shown.len(), current) {
            (0, _) => "No match in view. Push Enter to find the next one.".to_owned(),
            (count, Some(index)) => {
                format!("Match {} of {} in view", index + 1, grouped(count as u64))
            }
            (1, None) => "1 match in view".to_owned(),
            (count, None) => format!("{} matches in view", grouped(count as u64)),
        };
        (words, false)
    }

    pub(super) fn find_bar(&self) -> Option<Element<'_, Message>> {
        let finder = self.finder.as_ref()?;
        let input = text_input("Bases, for example GGATCC", &finder.text)
            .id(FIELD)
            .font(font::MONO)
            .size(12)
            .padding([3, 8])
            .width(FIELD_WIDTH)
            .style(style::locus)
            .on_input(|text| Message::Find(Finding::Edit(text)))
            .on_submit(Message::Find(Finding::Submit));
        let mut choices = row![text("Mismatches").size(11).style(style::muted)]
            .spacing(2)
            .align_y(Alignment::Center);
        for count in 0..=MOST_MISMATCHES {
            let look = if count == finder.mismatches {
                style::chosen
            } else {
                style::ghost
            };
            choices = choices.push(
                button(text(count.to_string()).size(11))
                    .padding([2, 7])
                    .style(look)
                    .on_press(Message::Find(Finding::Mismatches(count))),
            );
        }
        let (words, warning) = self.find_status(finder);
        let mut status = text(words).size(11).width(Length::Fill);
        if !warning {
            status = status.style(style::muted);
        }
        let below = tooltip::Position::Bottom;
        let previous = Message::Find(Finding::Step(false));
        let next = Message::Find(Finding::Step(true));
        let bar = row![
            text("Find").size(11).style(style::muted),
            input,
            choices,
            status,
            chrome::tip(
                small(Icon::Back, previous),
                "Previous match (Shift+Enter)",
                below
            ),
            chrome::tip(small(Icon::Forward, next), "Next match (Enter)", below),
            chrome::tip(
                small(Icon::Close, Message::Find(Finding::Close)),
                "Close (Escape)",
                below
            ),
        ]
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Center);
        Some(
            container(bar)
                .id("find-bar")
                .padding([0, 8])
                .height(BAR_HEIGHT)
                .into(),
        )
    }

    pub(super) fn find_lane<'a>(
        &'a self,
        session: &'a Session,
        panel: usize,
    ) -> Element<'a, Message> {
        let (contig, viewport) = session.view(panel);
        if panel > 0 && viewport.visible().len() > LIMIT {
            return container(text(TOO_WIDE).size(10).style(style::muted))
                .height(matches::HEIGHT)
                .padding([0, 8])
                .into();
        }
        let finder = self.finder.as_ref();
        let hits = finder
            .and_then(|finder| finder.results(panel, contig))
            .map_or(&[][..], |searched| searched.found.hits.as_slice());
        let current = finder
            .and_then(|finder| finder.current)
            .filter(|(_, at, _)| panel == 0 && *at == contig)
            .map(|(_, _, span)| span);
        let lane = scene::view(matches::scene(viewport, hits, current), matches::HEIGHT)
            .on_hover(move |pointer| Message::Find(Finding::Hover(panel, pointer)))
            .on_click(move |pointer| Message::Find(Finding::Click(panel, pointer)));
        container(lane).id(format!("find-lane-{panel}")).into()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use genie_core::search::{self, Hit, Pattern, Reach, Scanned};
    use genie_core::{Region, Strand, codon};
    use genie_formats::fai::FaiRecord;
    use iced::Point;
    use iced::keyboard::Modifiers;
    use iced_test::{selector, simulator};

    use super::{Finding, LANE, Swept, TOO_WIDE, search_bases};
    use crate::app::{Genie, Message};
    use crate::card;
    use crate::reference::Reference;
    use crate::scene::Pointer;
    use crate::shortcuts::Command;

    fn genie(lengths: &[u64]) -> Genie {
        let mut offset = 0;
        let records = lengths
            .iter()
            .enumerate()
            .map(|(index, length)| {
                let record = FaiRecord {
                    name: format!("chr{}", index + 1),
                    length: *length,
                    offset,
                    line_bases: 60,
                    line_width: 61,
                };
                offset += length + length / 60 + 8;
                record
            })
            .collect();
        let reference = Arc::new(Reference::fasta(PathBuf::from("ref.fa"), records).unwrap());
        let mut genie = Genie::default();
        genie.settings.motion = false;
        let _ = genie.update(Message::Opened(Ok(reference)));
        genie
    }

    fn go(genie: &mut Genie, locus: &str) {
        let _ = genie.update(Message::LocusEdited(locus.to_owned()));
        let _ = genie.update(Message::LocusSubmitted);
    }

    fn shows(genie: &Genie, text: &str) -> bool {
        simulator(genie.view()).find(text).is_ok()
    }

    fn find(genie: &mut Genie, text: &str) {
        let _ = genie.update(Message::Command(Command::Find));
        let _ = genie.update(Message::Find(Finding::Edit(text.to_owned())));
    }

    fn answer(genie: &mut Genie, panel: usize, sites: &[(u64, &str)]) {
        let finder = genie.finder.as_ref().unwrap();
        let (contig, region) = finder.wanted[panel].expect("a search runs");
        let mut bases = vec![b'A'; region.len() as usize];
        for (at, site) in sites {
            let offset = (at - region.start()) as usize;
            bases[offset..offset + site.len()].copy_from_slice(site.as_bytes());
        }
        let pattern = finder.pattern.as_ref().unwrap();
        let searched = search_bases(pattern, finder.mismatches, contig, region, bases.into());
        let message = Finding::Searched(finder.generation, panel, Ok(Arc::new(searched)));
        let _ = genie.update(Message::Find(message));
    }

    fn center(genie: &Genie) -> f64 {
        genie.session.as_ref().unwrap().destination().center()
    }

    fn generation(genie: &Genie) -> u64 {
        genie.finder.as_ref().unwrap().generation
    }

    #[test]
    fn ctrl_shift_f_opens_the_find_bar_and_escape_closes_it() {
        let mut genie = genie(&[1_000]);
        assert!(!shows(&genie, "Mismatches"));
        let _ = genie.update(Message::Command(Command::Find));
        {
            let mut ui = simulator(genie.view());
            assert!(ui.find(selector::id("find")).is_ok());
            assert!(ui.find("Mismatches").is_ok());
            assert!(ui.find(selector::id("find-lane-0")).is_ok());
        }
        let _ = genie.update(Message::Command(Command::Dismiss));
        assert!(genie.finder.is_none());
        let mut empty = Genie::default();
        let _ = empty.update(Message::Command(Command::Find));
        assert!(empty.finder.is_none());
    }

    #[test]
    fn typed_bases_search_the_view_and_count_the_matches() {
        let mut genie = genie(&[1_000]);
        find(&mut genie, "gacc");
        let finder = genie.finder.as_ref().unwrap();
        assert_eq!(finder.wanted[0], Some((0, Region::between(0, 1_000))));
        answer(&mut genie, 0, &[(100, "GACC"), (500, "GGTC")]);
        assert!(shows(&genie, "2 matches in view"));
        let hits = &genie
            .finder
            .as_ref()
            .unwrap()
            .results(0, 0)
            .unwrap()
            .found
            .hits;
        assert_eq!(hits[1].strand, Strand::Reverse);
        let stale = Finding::Searched(generation(&genie) - 1, 0, Err("old".to_owned()));
        let _ = genie.update(Message::Find(stale));
        assert!(genie.finder.as_ref().unwrap().failed.is_none());
        assert!(shows(&genie, "2 matches in view"));
    }

    #[test]
    fn wrong_letters_and_short_patterns_say_what_to_type() {
        let mut genie = genie(&[1_000]);
        find(&mut genie, "ACXT");
        let error = "X is not a base. Use A, C, G, T or IUPAC codes such as N, R and Y.";
        assert!(shows(&genie, error));
        assert!(
            genie
                .finder
                .as_ref()
                .unwrap()
                .wanted
                .iter()
                .all(Option::is_none)
        );
        let _ = genie.update(Message::Find(Finding::Edit("GACCTA".to_owned())));
        let _ = genie.update(Message::Find(Finding::Mismatches(2)));
        assert!(shows(&genie, "Type 8 bases or more to allow 2 mismatches."));
        let _ = genie.update(Message::Find(Finding::Mismatches(1)));
        assert!(genie.finder.as_ref().unwrap().wanted[0].is_some());
    }

    #[test]
    fn mismatches_find_near_matches_too() {
        let mut genie = genie(&[1_000]);
        find(&mut genie, "GACCTTAA");
        let sites = [(100, "GACCTTAA"), (300, "GACCATAA")];
        answer(&mut genie, 0, &sites);
        assert!(shows(&genie, "1 match in view"));
        let _ = genie.update(Message::Find(Finding::Mismatches(1)));
        assert!(shows(&genie, "Searching…"));
        answer(&mut genie, 0, &sites);
        assert!(shows(&genie, "2 matches in view"));
    }

    #[test]
    fn enter_goes_to_the_next_match_and_shift_enter_back() {
        let mut genie = genie(&[10_000]);
        go(&mut genie, "chr1:1-200");
        find(&mut genie, "GACC");
        answer(
            &mut genie,
            0,
            &[(150, "GACC"), (600, "GACC"), (900, "GACC")],
        );
        let _ = genie.update(Message::Find(Finding::Submit));
        assert!((center(&genie) - 152.5).abs() < 1e-6, "{}", center(&genie));
        assert!(shows(&genie, "Match 1 of 1 in view"));
        let _ = genie.update(Message::Find(Finding::Step(true)));
        assert!((center(&genie) - 602.5).abs() < 1e-6, "{}", center(&genie));
        let _ = genie.update(Message::Modifiers(Modifiers::SHIFT));
        let _ = genie.update(Message::Find(Finding::Submit));
        assert!((center(&genie) - 152.5).abs() < 1e-6, "{}", center(&genie));
    }

    #[test]
    fn with_no_match_near_the_search_reads_on() {
        let mut genie = genie(&[50_000_000, 1_000]);
        go(&mut genie, "chr1:1-1,000");
        find(&mut genie, "GACC");
        answer(&mut genie, 0, &[]);
        assert!(shows(
            &genie,
            "No match in view. Push Enter to find the next one."
        ));
        let _ = genie.update(Message::Find(Finding::Submit));
        assert!(genie.finder.as_ref().unwrap().scanning);
        assert!(shows(&genie, "Searching…"));
        let none = |from, reached| Swept {
            contig: 0,
            forward: true,
            from,
            scanned: Scanned { hit: None, reached },
        };
        let swept = Finding::Scanned(generation(&genie), Ok(none(10_997, 20_010_997)));
        let _ = genie.update(Message::Find(swept));
        assert!(shows(
            &genie,
            "No match in chr1:10,998-20,010,997. Push Enter again to search on."
        ));
        let _ = genie.update(Message::Find(Finding::Submit));
        assert!(genie.finder.as_ref().unwrap().scanning);
        let swept = Finding::Scanned(generation(&genie), Ok(none(20_010_997, 50_000_000)));
        let _ = genie.update(Message::Find(swept));
        assert!(shows(
            &genie,
            "No match from chr1:20,010,998 to the end of chr1. Push Enter to go on in chr2."
        ));
        let hit = Hit {
            span: Region::between(30_000_000, 30_000_004),
            strand: Strand::Forward,
            mismatches: Vec::new(),
        };
        let found = Swept {
            contig: 0,
            forward: true,
            from: 20_010_997,
            scanned: Scanned {
                hit: Some(hit),
                reached: 30_000_000,
            },
        };
        let _ = genie.update(Message::Find(Finding::Submit));
        let _ = genie.update(Message::Find(Finding::Scanned(
            generation(&genie),
            Ok(found),
        )));
        assert!(
            (center(&genie) - 30_000_002.5).abs() < 1e-6,
            "{}",
            center(&genie)
        );
        assert!(genie.finder.as_ref().unwrap().note.is_none());
    }

    #[test]
    fn a_match_shows_a_card_with_its_bases() {
        let mut genie = genie(&[1_000]);
        go(&mut genie, "chr1:1-100");
        find(&mut genie, "GACCTA");
        let _ = genie.update(Message::Find(Finding::Mismatches(1)));
        answer(&mut genie, 0, &[(20, "GACCTA"), (60, "TAGGTA")]);
        let (_, viewport) = genie.session.as_ref().unwrap().view(0);
        let x = viewport.to_px(62.0) as f32;
        let pointer = Pointer {
            local: Point::new(x, 6.0),
            at: Point::new(x, 120.0),
        };
        let _ = genie.update(Message::Find(Finding::Hover(0, Some(pointer))));
        let hover = genie.hover.as_ref().expect("a card");
        assert_eq!(hover.track, LANE);
        assert_eq!(
            card::plain(hover),
            "GACCTA\nchr1:61-66\n6 bp · reverse strand · 1 mismatch\ntACCTA"
        );
        let _ = genie.update(Message::Find(Finding::Click(0, pointer)));
        assert!(genie.pinned.is_some());
        let _ = genie.update(Message::Find(Finding::Hover(0, None)));
        assert!(genie.hover.is_none());
        let _ = genie.update(Message::Find(Finding::Close));
        assert!(genie.pinned.is_none());
    }

    #[test]
    fn matches_come_from_a_real_reference_file() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../genie-formats/tests/data/reference.fa.gz");
        let reference = crate::reference::open(path, None, &mut |_| {}).unwrap();
        let region = Region::between(70_000, 110_000);
        let bases: Arc<[u8]> = reference.fetch(0, region).unwrap().into();
        let primer = String::from_utf8(bases[25_000..25_020].to_vec()).unwrap();
        assert!(
            primer.bytes().all(|base| b"ACGTacgt".contains(&base)),
            "{primer}"
        );
        let place = Region::between(95_000, 95_020);
        let pattern = Pattern::parse(&primer).unwrap();
        let forward = search_bases(&pattern, 0, 0, region, Arc::clone(&bases));
        assert!(
            forward
                .found
                .hits
                .iter()
                .any(|hit| hit.span == place && hit.strand == Strand::Forward)
        );
        let flipped: String = primer
            .bytes()
            .rev()
            .map(|base| char::from(codon::complement(base)))
            .collect();
        let reverse = search_bases(&Pattern::parse(&flipped).unwrap(), 0, 0, region, bases);
        assert!(
            reverse
                .found
                .hits
                .iter()
                .any(|hit| hit.span == place && hit.strand == Strand::Reverse)
        );
        let reach = Reach {
            from: 0,
            forward: true,
            length: 120_000,
            budget: 120_000,
            chunk: 7_000,
        };
        let scanned =
            search::scan(&pattern, 0, reach, |window| reference.fetch(0, window)).unwrap();
        assert_eq!(scanned.hit.map(|hit| hit.span), Some(place));
    }

    #[test]
    fn wide_views_ask_to_zoom_in() {
        let mut genie = genie(&[50_000_000]);
        find(&mut genie, "GACC");
        assert!(
            genie
                .finder
                .as_ref()
                .unwrap()
                .wanted
                .iter()
                .all(Option::is_none)
        );
        assert!(shows(&genie, TOO_WIDE));
    }

    #[test]
    fn another_reference_forgets_the_matches() {
        let mut genie = genie(&[1_000]);
        find(&mut genie, "GACC");
        answer(&mut genie, 0, &[(100, "GACC")]);
        let other = genie.session.as_ref().unwrap().reference.clone();
        let _ = genie.update(Message::Opened(Ok(other)));
        let finder = genie.finder.as_ref().unwrap();
        assert!(finder.results(0, 0).is_none());
        assert_eq!(finder.text, "GACC");
    }
}
