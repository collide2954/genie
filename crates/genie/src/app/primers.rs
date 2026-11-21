//! Primer design: the dialog of a target, the pairs that genie designs around it, and the check
//! that each pair makes only its own product in the genome.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use genie_core::binding::{self, Bind, Binder};
use genie_core::pcr::LONGEST;
use genie_core::primer::{self, PCR, Pair, QPCR, SANGER, Settings};
use genie_core::review::Reviews;
use genie_core::{Locus, Region, Span, Target, grouped};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::widget::{
    Column, button, center, column, container, opaque, row, space, text, text_input,
};
use iced::{Alignment, Element, Length, Task};

use crate::card::Detail;
use crate::icon::{self, Icon};
use crate::reference::Reference;
use crate::track::{self, Content, Lift, Options, Scope, Source};
use crate::{font, style};

use super::find::Finding;
use super::{Genie, Message, Popup};

const WIDTH: f32 = 640.0;
/// The pairs that genie checks in the genome, the best first.
const CHECKED: usize = 20;
/// The pairs that the dialog shows.
const SHOWN: usize = 5;
/// The bases of a chromosome that genie reads at one time for the check.
const CHUNK: u64 = 8 << 20;
/// The bases on each side of the target that genie checks for a reference on the web.
const AROUND: u64 = 500_000;
/// The most places where one primer may bind: more, and the primer binds too often.
const MOST_BINDS: usize = 10_000;
const STOPPED: &str = "Stopped.";
const PRESETS: [(&str, (u64, u64)); 3] = [
    ("PCR 150–500", PCR),
    ("Sanger 400–800", SANGER),
    ("qPCR 70–150", QPCR),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Priming {
    /// Opens the dialog for the variant of the open card.
    Card,
    /// Opens the dialog for a region of the region list, or for the view.
    Region(Option<u64>),
    /// Opens the dialog for a target on a contig.
    Open(usize, Region),
    /// The shortest and the longest product of a preset.
    Size(u64, u64),
    Shortest(String),
    Longest(String),
    Design,
    /// What the work does now, for the work with this number.
    Progress(u64, String),
    /// The pairs of the work with this number, each with its products in the genome.
    Done(u64, Result<Vec<Checked>, String>),
    /// Shows the product of a pair in the tracks.
    Show(usize),
    /// Copies the primers of a pair.
    Copy(usize),
}

/// A pair and its check: the number of products that it makes in the checked part of the
/// genome, and whether one of its primers binds too often to count.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub pair: Pair,
    pub products: usize,
    pub crowded: bool,
}

/// The dialog: its target, the sizes of the products, the work that runs, and the pairs.
#[derive(Debug)]
pub(super) struct PrimerDialog {
    contig: usize,
    target: Region,
    sizes: (String, String),
    work: u64,
    progress: Option<String>,
    stop: Arc<AtomicBool>,
    pairs: Vec<Checked>,
    note: Option<String>,
    /// Whether the check covers the whole genome, or only the bases around the target.
    whole: bool,
}

/// A variant track whose variants primers stay off: its file, its lift, the name of the contig
/// in the file, the rules of its filter, and its review marks for the filter.
type Masker = (
    Arc<Source>,
    Option<Arc<Lift>>,
    String,
    Vec<genie_formats::fields::Rule>,
    Arc<Reviews>,
);

/// What the work needs: the reference, the target, the variant tracks that mask places, and
/// the settings.
struct Job {
    reference: Arc<Reference>,
    contig: usize,
    target: Region,
    variants: Vec<Masker>,
    settings: Settings,
    whole: bool,
}

impl Genie {
    pub(super) fn priming(&mut self, priming: Priming) -> Task<Message> {
        match priming {
            Priming::Card => {
                let target = self.pinned.as_ref().and_then(|card| {
                    let Detail::Variant { variant, .. } = &card.detail else {
                        return None;
                    };
                    let contig = self
                        .session
                        .as_ref()?
                        .reference
                        .assembly
                        .find(&card.contig)?;
                    Some((contig, variant.span))
                });
                if let Some((contig, span)) = target {
                    return self.priming(Priming::Open(contig, span));
                }
            }
            Priming::Region(region) => {
                if let Some((contig, span)) = self.target(region) {
                    return self.priming(Priming::Open(contig, span));
                }
            }
            Priming::Open(contig, target) => return self.open_primers(contig, target),
            Priming::Size(shortest, longest) => {
                if let Some(dialog) = &mut self.primers {
                    dialog.sizes = (shortest.to_string(), longest.to_string());
                }
                return self.design_primers();
            }
            Priming::Shortest(text) => {
                if let Some(dialog) = &mut self.primers {
                    dialog.sizes.0 = text;
                }
            }
            Priming::Longest(text) => {
                if let Some(dialog) = &mut self.primers {
                    dialog.sizes.1 = text;
                }
            }
            Priming::Design => return self.design_primers(),
            Priming::Progress(work, step) => {
                if let Some(dialog) = self.primers.as_mut().filter(|dialog| dialog.work == work) {
                    dialog.progress = Some(step);
                }
            }
            Priming::Done(work, result) => self.primers_done(work, result),
            Priming::Show(index) => return self.show_pair(index),
            Priming::Copy(index) => return self.copy_pair(index),
        }
        Task::none()
    }

    fn open_primers(&mut self, contig: usize, target: Region) -> Task<Message> {
        self.close_menus();
        let Some(session) = &self.session else {
            return Task::none();
        };
        let whole = !session.reference.location.is_remote();
        self.primers = Some(PrimerDialog {
            contig,
            target,
            sizes: (PCR.0.to_string(), PCR.1.to_string()),
            work: 0,
            progress: None,
            stop: Arc::new(AtomicBool::new(false)),
            pairs: Vec::new(),
            note: None,
            whole,
        });
        self.popup = Some(Popup::Primers);
        self.design_primers()
    }

    /// Starts the work of the dialog with its sizes: the design of the pairs, then their check.
    fn design_primers(&mut self) -> Task<Message> {
        let (Some(dialog), Some(session)) = (&mut self.primers, &self.session) else {
            return Task::none();
        };
        dialog.stop.store(true, Ordering::Relaxed);
        dialog.pairs.clear();
        dialog.progress = None;
        let sizes = (
            dialog.sizes.0.trim().parse::<u64>(),
            dialog.sizes.1.trim().parse::<u64>(),
        );
        let (Ok(shortest), Ok(longest)) = sizes else {
            dialog.note = Some(BAD_SIZES.to_owned());
            return Task::none();
        };
        if shortest >= longest {
            dialog.note = Some(BAD_SIZES.to_owned());
            return Task::none();
        }
        let settings = Settings {
            product: (shortest, longest),
            ..Settings::default()
        };
        if dialog.target.len() + 2 * settings.lengths[0] as u64 > longest {
            dialog.note = Some(format!(
                "The target is too long for products of {} to {} bp.",
                grouped(shortest),
                grouped(longest)
            ));
            return Task::none();
        }
        dialog.note = None;
        dialog.work += 1;
        dialog.stop = Arc::new(AtomicBool::new(false));
        dialog.progress = Some(DESIGNING.to_owned());
        let contig = dialog.contig;
        let variants = session
            .tracks
            .iter()
            .filter(|track| track.source.has_variants() && !track.hidden)
            .filter_map(|track| {
                let name = track.contigs.get(contig)?.clone()?;
                let source = Arc::clone(&track.source);
                let (rules, reviews) = (track.filter.rules.clone(), Arc::clone(&track.reviews));
                Some((source, track.lift.clone(), name, rules, reviews))
            })
            .collect();
        let job = Job {
            reference: Arc::clone(&session.reference),
            contig,
            target: dialog.target,
            variants,
            settings,
            whole: dialog.whole,
        };
        let (work, stop) = (dialog.work, Arc::clone(&dialog.stop));
        let events = iced::stream::channel(16, async move |mut output| {
            let (sender, mut receiver) = mpsc::unbounded();
            thread::spawn(move || {
                let progress = sender.clone();
                let mut report = move |step: String| {
                    let _ = progress.unbounded_send(Priming::Progress(work, step));
                };
                let result = design_and_check(&job, &mut report, &stop);
                let _ = sender.unbounded_send(Priming::Done(work, result));
            });
            while let Some(event) = receiver.next().await {
                let _ = output.send(event).await;
            }
        });
        Task::run(events, Message::Primers)
    }

    /// Keeps the pairs of the work: the pairs that make only their own product first.
    fn primers_done(&mut self, work: u64, result: Result<Vec<Checked>, String>) {
        let Some(dialog) = self.primers.as_mut().filter(|dialog| dialog.work == work) else {
            return;
        };
        dialog.progress = None;
        match result {
            Ok(checked) if checked.is_empty() => dialog.note = Some(NO_PAIR.to_owned()),
            Ok(checked) => {
                let alone = |pair: &Checked| pair.products == 1 && !pair.crowded;
                let (good, other): (Vec<Checked>, Vec<Checked>) =
                    checked.into_iter().partition(alone);
                dialog.note = good.is_empty().then(|| OTHER_PRODUCTS.to_owned());
                dialog.pairs = if good.is_empty() { other } else { good };
                dialog.pairs.truncate(SHOWN);
            }
            Err(error) if error == STOPPED => {}
            Err(error) => dialog.note = Some(error),
        }
    }

    /// Shows the product of a pair in the tracks, with its primers in the find bar.
    fn show_pair(&mut self, index: usize) -> Task<Message> {
        let Some((contig, pair)) = self
            .primers
            .as_ref()
            .and_then(|dialog| Some((dialog.contig, dialog.pairs.get(index)?.pair.clone())))
        else {
            return Task::none();
        };
        let margin = pair.product.len() / 5;
        let shown = Region::between(
            pair.product.start().saturating_sub(margin),
            pair.product.end() + margin,
        );
        self.close_menus();
        let go = self.go_to(Target {
            contig,
            span: Span::Region(shown),
        });
        let open = self.open_finder();
        let tasks = [
            go,
            open,
            self.find(Finding::Primers(true)),
            self.find(Finding::Edit(pair.left.bases)),
            self.find(Finding::Reverse(pair.right.bases)),
        ];
        Task::batch(tasks)
    }

    fn copy_pair(&mut self, index: usize) -> Task<Message> {
        let (Some(dialog), Some(session)) = (&self.primers, &self.session) else {
            return Task::none();
        };
        let Some(checked) = dialog.pairs.get(index) else {
            return Task::none();
        };
        let locus = Locus {
            name: session.reference.assembly.contigs()[dialog.contig]
                .name
                .clone(),
            span: Span::Region(checked.pair.product),
        };
        let text = format!(
            "{locus} F\t{}\n{locus} R\t{}\n",
            checked.pair.left.bases, checked.pair.right.bases
        );
        let notice = format!("Copied the primers of pair {}.", index + 1);
        Task::batch([iced::clipboard::write(text), self.inform(notice)])
    }

    /// Stops the work of the dialog of primers, as when the dialog closes.
    pub(super) fn stop_primers(&self) {
        if let Some(dialog) = &self.primers {
            dialog.stop.store(true, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    pub(super) fn primer_work(&self) -> u64 {
        self.primers.as_ref().map_or(0, |dialog| dialog.work)
    }

    pub(super) fn primer_dialog(&self) -> Option<Element<'_, Message>> {
        if self.popup != Some(Popup::Primers) {
            return None;
        }
        let (dialog, session) = (self.primers.as_ref()?, self.session.as_ref()?);
        let locus = Locus {
            name: session.reference.assembly.contigs()[dialog.contig]
                .name
                .clone(),
            span: Span::Region(dialog.target),
        };
        let close = button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::MenuDismissed);
        let heading = row![
            text(format!("Primers for {locus}"))
                .font(font::UI_STRONG)
                .size(13),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center);
        let mut sizes = row![].spacing(4).align_y(Alignment::Center);
        for (label, (shortest, longest)) in PRESETS {
            let chosen = dialog.sizes == (shortest.to_string(), longest.to_string());
            sizes = sizes.push(
                button(text(label).size(12))
                    .padding([3, 8])
                    .style(if chosen { style::chosen } else { style::ghost })
                    .on_press(Message::Primers(Priming::Size(shortest, longest))),
            );
        }
        let field = |value: &str, edit: fn(String) -> Priming| {
            text_input("bp", value)
                .width(56)
                .size(12)
                .padding([3, 6])
                .style(style::locus)
                .on_input(move |text| Message::Primers(edit(text)))
                .on_submit(Message::Primers(Priming::Design))
        };
        sizes = sizes
            .push(space::horizontal())
            .push(field(&dialog.sizes.0, Priming::Shortest))
            .push(text("to").size(12).style(style::muted))
            .push(field(&dialog.sizes.1, Priming::Longest))
            .push(text("bp").size(12).style(style::muted))
            .push(
                button(text("Design").size(12))
                    .padding([4, 12])
                    .style(style::primary)
                    .on_press(Message::Primers(Priming::Design)),
            );
        let mut body = column![heading, sizes].spacing(10);
        if let Some(words) = dialog.progress.as_deref().or(dialog.note.as_deref()) {
            body = body.push(text(words.to_owned()).size(11).style(style::muted));
        }
        body = body.push(pair_rows(dialog));
        let scope = if dialog.whole { WHOLE } else { AROUND_ONLY };
        body = body.push(text(scope).size(11).style(style::muted));
        let card = container(body)
            .id("primer-dialog")
            .width(Length::Fill)
            .max_width(WIDTH)
            .padding(16)
            .style(style::menu);
        Some(center(opaque(card)).width(Length::Fill).into())
    }
}

const BAD_SIZES: &str = "Type two product sizes in bases. The second must be larger.";
const DESIGNING: &str = "Designing primers…";
const NO_PAIR: &str = "No pair keeps to the rules. Try other product sizes.";
const OTHER_PRODUCTS: &str = "Each pair also makes other products. These are the best pairs; check them before you use them.";
const WHOLE: &str = "genie checks each pair for other products up to 4 kb in the whole genome.";
const AROUND_ONLY: &str =
    "The reference is on the web, so genie checks 1 Mb around the target for other products.";

/// The rows of the pairs of the dialog: the product, the products in the genome, and each
/// primer with its length, its Tm and its GC content.
fn pair_rows(dialog: &PrimerDialog) -> Column<'static, Message> {
    let mut rows = Column::new().spacing(8);
    for (index, checked) in dialog.pairs.iter().enumerate() {
        let products = if checked.crowded {
            "a primer binds too often to count".to_owned()
        } else {
            let place = if dialog.whole {
                "in the genome"
            } else {
                "in 1 Mb"
            };
            match checked.products {
                1 => format!("1 product {place}"),
                count => format!("{count} products {place}"),
            }
        };
        let action = |label: &'static str, priming: Priming| {
            button(text(label).size(11))
                .padding([1, 6])
                .style(style::ghost)
                .on_press(Message::Primers(priming))
        };
        let pair = &checked.pair;
        let title = row![
            text(format!("{}. {} bp", index + 1, grouped(pair.product.len())))
                .font(font::UI_STRONG)
                .size(12),
            text(products).size(11).style(style::muted),
            space::horizontal(),
            action("Show", Priming::Show(index)),
            action("Copy", Priming::Copy(index)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let line = |side: &'static str, primer: &primer::Primer| {
            row![
                text(side).size(11).font(font::MONO).style(style::muted),
                text(primer.bases.clone()).size(12).font(font::MONO),
                space::horizontal(),
                text(format!(
                    "{} nt · {:.1} °C · {:.0} % GC",
                    primer.bases.len(),
                    primer.tm,
                    primer.gc
                ))
                .size(11)
                .style(style::muted),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        };
        rows = rows.push(column![title, line("F", &pair.left), line("R", &pair.right)].spacing(2));
    }
    rows
}

/// The places of the variants of the tracks in a region, which primers stay off. A track with
/// a filter masks only the variants that the filter keeps.
fn masked(job: &Job, region: Region) -> Vec<u64> {
    let length = job.reference.assembly.contigs()[job.contig].length;
    let mut places = Vec::new();
    for (source, lift, name, rules, reviews) in &job.variants {
        let scope = Scope {
            step: 1,
            detail: true,
            rules,
            length,
            phased: &[],
            cells: None,
            depths: None,
        };
        let loaded = track::load_track(
            source,
            lift.as_deref(),
            (name, region),
            Options::default(),
            scope,
            |_| Err(String::new()),
        );
        let Ok(loaded) = loaded else {
            continue;
        };
        let Content::Variants(calls) = &loaded.content else {
            continue;
        };
        for variant in &calls.variants {
            let fields = track::variant_fields(source, variant, &[], reviews.of(name, variant));
            if !genie_formats::fields::passes(rules, &fields) {
                continue;
            }
            let start = variant.span.start().max(region.start());
            let end = variant.span.end().min(region.end()).max(start + 1);
            places.extend(start..end);
        }
    }
    places
}

/// Designs the pairs of a job and checks them in the genome, with a report of each step.
fn design_and_check(
    job: &Job,
    report: &mut dyn FnMut(String),
    stop: &AtomicBool,
) -> Result<Vec<Checked>, String> {
    let stopped = || Err(STOPPED.to_owned());
    let assembly = &job.reference.assembly;
    let length = assembly.contigs()[job.contig].length;
    let longest = job.settings.product.1;
    let template = Region::between(
        job.target.end().saturating_sub(longest),
        job.target.start().saturating_add(longest),
    )
    .clamp_to(length);
    let bases = job.reference.fetch(job.contig, template)?;
    let masked = masked(job, template);
    let pairs = primer::design(
        &bases,
        template.start(),
        job.target,
        &masked,
        &job.settings,
        CHECKED,
    );
    if stop.load(Ordering::Relaxed) {
        return stopped();
    }
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let primers: Vec<&str> = pairs
        .iter()
        .flat_map(|pair| [pair.left.bases.as_str(), pair.right.bases.as_str()])
        .collect();
    let binder = Binder::new(&primers);
    let parts: Vec<(usize, Region)> = if job.whole {
        let contigs = assembly.contigs().iter().enumerate();
        contigs
            .map(|(index, contig)| (index, Region::between(0, contig.length)))
            .collect()
    } else {
        let around = Region::between(
            job.target.start().saturating_sub(AROUND),
            job.target.end().saturating_add(AROUND),
        );
        vec![(job.contig, around.clamp_to(length))]
    };
    let (mut products, mut counts) = (vec![0; pairs.len()], vec![0; primers.len()]);
    let total = parts.len();
    for (step, (contig, region)) in parts.into_iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            return stopped();
        }
        let name = &assembly.contigs()[contig].name;
        report(format!("Checking {name} ({} of {total})…", step + 1));
        let size = assembly.contigs()[contig].length;
        let (mut binds, mut kept): (Vec<Bind>, Vec<usize>) = (Vec::new(), vec![0; primers.len()]);
        let mut at = region.start();
        while at < region.end() {
            if stop.load(Ordering::Relaxed) {
                return stopped();
            }
            let own = Region::between(at, (at + CHUNK).min(region.end()));
            let read = Region::between(
                own.start().saturating_sub(binder.overlap()),
                own.end() + binder.overlap(),
            )
            .clamp_to(size);
            let chunk = job.reference.fetch(contig, read)?;
            for bind in binder.scan(&chunk, read.start(), own) {
                counts[bind.primer] += 1;
                if kept[bind.primer] < MOST_BINDS {
                    kept[bind.primer] += 1;
                    binds.push(bind);
                }
            }
            at = own.end();
        }
        binds.sort_by_key(|bind| bind.span.start());
        for (index, count) in products.iter_mut().enumerate() {
            *count += binding::products(&binds, (2 * index, 2 * index + 1), LONGEST).len();
        }
    }
    Ok(pairs
        .into_iter()
        .enumerate()
        .map(|(index, pair)| Checked {
            pair,
            products: products[index],
            crowded: counts[2 * index] > MOST_BINDS || counts[2 * index + 1] > MOST_BINDS,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use genie_core::Region;
    use genie_core::primer::Settings;

    use super::{Job, Priming, design_and_check as work};
    use crate::app::{Genie, Message};
    use crate::location::Location;
    use crate::scratch::Scratch;

    /// Bases that look random, with half G and C.
    fn bases(length: usize, seed: u64) -> String {
        let mut state = seed;
        (0..length)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                char::from(b"ACGT"[((state >> 33) % 4) as usize])
            })
            .collect()
    }

    fn open_reference(
        scratch: &Scratch,
        name: &str,
        text: &str,
    ) -> Arc<crate::reference::Reference> {
        let path = scratch.file(name, text);
        Arc::new(crate::reference::open(path, None, &mut |_| {}).unwrap())
    }

    #[test]
    fn the_work_designs_pairs_and_counts_their_products() {
        let scratch = Scratch::new("primers-work");
        let chr1 = bases(3_000, 3);
        let chr2 = bases(2_000, 5);
        let reference = open_reference(
            &scratch,
            "ref.fa",
            &format!(">chr1\n{chr1}\n>chr2\n{chr2}\n"),
        );
        let job = Job {
            reference: Arc::clone(&reference),
            contig: 0,
            target: Region::between(1_400, 1_500),
            variants: Vec::new(),
            settings: Settings::default(),
            whole: true,
        };
        let mut steps = Vec::new();
        let checked = work(&job, &mut |step| steps.push(step), &AtomicBool::new(false)).unwrap();
        assert!(checked.len() >= 5, "{}", checked.len());
        assert!(
            checked
                .iter()
                .all(|pair| pair.products == 1 && !pair.crowded)
        );
        assert_eq!(
            steps,
            ["Checking chr1 (1 of 2)…", "Checking chr2 (2 of 2)…"]
        );
        let best = &checked[0].pair;
        let copy = format!(
            ">chr1\n{chr1}\n>chr2\n{}{}{}\n",
            &chr2[..500],
            &chr1[best.product.start() as usize..best.product.end() as usize],
            &chr2[500 + best.product.len() as usize..]
        );
        let twice = open_reference(&scratch, "twice.fa", &copy);
        let job = Job {
            reference: twice,
            ..job
        };
        let checked = work(&job, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        let same = checked.iter().find(|pair| pair.pair == *best).unwrap();
        assert_eq!(same.products, 2);
        let stop = AtomicBool::new(true);
        assert_eq!(work(&job, &mut |_| {}, &stop), Err("Stopped.".to_owned()));
    }

    #[test]
    fn variants_of_open_tracks_keep_primers_away() {
        let scratch = Scratch::new("primers-variants");
        let chr1 = bases(3_000, 9);
        let reference = open_reference(&scratch, "ref.fa", &format!(">chr1\n{chr1}\n"));
        let mut job = Job {
            reference,
            contig: 0,
            target: Region::between(1_400, 1_500),
            variants: Vec::new(),
            settings: Settings::default(),
            whole: false,
        };
        let best = work(&job, &mut |_| {}, &AtomicBool::new(false)).unwrap()[0]
            .pair
            .clone();
        let place = best.left.span.start() + 3;
        let vcf = scratch.file(
            "calls.vcf",
            format!(
                "##fileformat=VCFv4.2\n#CHROM\tPOS\tID\tREF\tALT\tQUAL\tFILTER\tINFO\nchr1\t{}\t.\tA\tG\t.\tPASS\t.\n",
                place + 1
            ),
        );
        let source = Arc::new(crate::track::open(Location::File(vcf)).unwrap());
        job.variants = vec![(source, None, "chr1".to_owned(), Vec::new(), Arc::default())];
        let pairs = work(&job, &mut |_| {}, &AtomicBool::new(false)).unwrap();
        assert!(
            pairs
                .iter()
                .all(|pair| !pair.pair.left.span.contains(place))
        );
    }

    fn opened(scratch: &Scratch) -> Genie {
        let reference = open_reference(scratch, "ref.fa", &format!(">chr1\n{}\n", bases(3_000, 3)));
        let mut genie = Genie::default();
        let _ = genie.update(Message::Opened(Ok(reference)));
        genie
    }

    fn shows(genie: &Genie, words: &str) -> bool {
        iced_test::simulator(genie.view()).find(words).is_ok()
    }

    #[test]
    fn the_dialog_shows_the_best_pairs_and_shows_a_product() {
        let scratch = Scratch::new("primers-dialog");
        let mut genie = opened(&scratch);
        let _ = genie.update(Message::Primers(Priming::Open(
            0,
            Region::between(1_400, 1_500),
        )));
        assert!(shows(&genie, "Primers for chr1:1,401-1,500"));
        assert!(shows(&genie, "PCR 150–500"));
        let work = genie.primer_work();
        let reference = Arc::clone(&genie.session.as_ref().unwrap().reference);
        let job = Job {
            reference,
            contig: 0,
            target: Region::between(1_400, 1_500),
            variants: Vec::new(),
            settings: Settings::default(),
            whole: true,
        };
        let _ = genie.update(Message::Primers(Priming::Progress(
            work,
            "Checking chr1 (1 of 1)…".to_owned(),
        )));
        assert!(shows(&genie, "Checking chr1 (1 of 1)…"));
        let checked = work_now(&job);
        let left = checked[0].pair.left.bases.clone();
        let _ = genie.update(Message::Primers(Priming::Done(work, Ok(checked))));
        assert!(shows(&genie, &left));
        assert!(shows(&genie, "1 product in the genome"));
        let _ = genie.update(Message::Primers(Priming::Show(0)));
        assert!(genie.popup.is_none());
        assert!(genie.finder.is_some());
        let _ = genie.update(Message::Primers(Priming::Open(
            0,
            Region::between(1_400, 1_500),
        )));
        let _ = genie.update(Message::Primers(Priming::Size(400, 800)));
        assert!(shows(&genie, "Primers for chr1:1,401-1,500"));
        assert!(shows(&genie, "Designing primers…"));
        let _ = genie.update(Message::Primers(Priming::Open(
            0,
            Region::between(0, 2_000),
        )));
        let _ = genie.update(Message::Primers(Priming::Design));
        assert!(shows(
            &genie,
            "The target is too long for products of 150 to 500 bp."
        ));
    }

    fn work_now(job: &Job) -> Vec<super::Checked> {
        work(job, &mut |_| {}, &AtomicBool::new(false)).unwrap()
    }
}
