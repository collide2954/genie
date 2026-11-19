use genie_render::scene::{Align, Primitive};
use genie_render::{alignments, bands, ruler, sequence, translation};
use iced::Color;
use iced::theme::Base;

use crate::scene::{CHIP, CHIP_EDGE};
use crate::svg::{self, Face, Part, Svg};
use crate::theme::{DataColors, Tokens, data, tokens};
use crate::track::Track;

use super::export::{
    BAR_INSET, CAPTION_HEIGHT, CAPTION_SIZE, TITLE_SIZE, band_rows, one_caption, panel_bands,
};
use super::view::{
    BASE_HEIGHT, Body, EMPTY_HEIGHT, HEADER_HEIGHT, LABEL_SIZE, RULER_HEIGHT, body, members, shapes,
};
use super::{Genie, PANEL_GAP, Session, whole};

const LINE_HEIGHT: f32 = 1.3;
const HINT_SIZE: f32 = 11.0;
const GAP: f32 = 6.0;
const LOCUS_INSET: f32 = 10.0;

struct Page<'a> {
    svg: Svg,
    width: f32,
    /// The left edge and the width of each panel.
    panels: Vec<(f32, f32)>,
    colors: (&'a Tokens, &'a DataColors),
}

impl Page<'_> {
    fn hairline(&mut self, y: f32) {
        self.svg.rect(0.0, y, self.width, 1.0, self.colors.0.line);
    }

    fn gaps(&mut self, y: f32, height: f32) {
        for panel in 1..self.panels.len() {
            let x = self.panels[panel].0 - PANEL_GAP;
            self.svg.rect(x, y, PANEL_GAP, height, self.colors.0.line);
        }
    }

    fn chip(&mut self, (x, y): (f32, f32), names: &[(&str, Color)]) {
        let widths: Vec<f32> = names
            .iter()
            .map(|(name, _)| svg::width(name, LABEL_SIZE, Face::Ui))
            .collect();
        let gaps = GAP * names.len().saturating_sub(1) as f32;
        let width = widths.iter().sum::<f32>() + gaps + 8.0;
        let height = LABEL_SIZE * LINE_HEIGHT;
        let fill = self.colors.0.canvas.scale_alpha(CHIP);
        self.svg.rounded((x, y, width, height), 3.0, fill);
        let mut at = x + 4.0;
        for ((name, color), size) in names.iter().zip(widths) {
            let center = (at, y + height / 2.0);
            self.svg
                .text(center, name, LABEL_SIZE, Face::Ui, *color, Align::Start);
            at += size + GAP;
        }
    }

    fn draw(&mut self, primitives: &[Primitive], at: (f32, f32)) {
        svg::draw(&mut self.svg, primitives, at, self.colors);
    }

    fn draw_in(&mut self, panel: usize, (y, height): (f32, f32), parts: &[(&[Primitive], f32)]) {
        let (x, width) = self.panels[panel];
        self.svg.clip((x, y, width, height));
        for (primitives, top) in parts {
            self.draw(primitives, (x, *top));
        }
        self.svg.unclip();
    }
}

impl Page<'_> {
    /// The first line: the title, the reference and the locus, and the scale bar.
    fn caption(&mut self, session: &Session) {
        let tokens = self.colors.0;
        let (label, locus) = (session.reference.label(), session.locus());
        let mut caption: Vec<Part> = Vec::new();
        if let Some(title) = session.figure.title() {
            caption.push((title, Face::UiStrong, tokens.text, TITLE_SIZE));
            caption.push(("·", Face::Ui, tokens.muted, CAPTION_SIZE));
        }
        caption.push((label.as_str(), Face::Ui, tokens.text, CAPTION_SIZE));
        let single = one_caption(session);
        if single {
            caption.push(("·", Face::Ui, tokens.muted, CAPTION_SIZE));
            caption.push((locus.as_str(), Face::Mono, tokens.muted, CAPTION_SIZE));
        }
        self.svg
            .spans((8.0, CAPTION_HEIGHT / 2.0), CAPTION_SIZE, &caption, GAP);
        if single && session.figure.scale {
            let (_, viewport) = session.view(0);
            let right = self.width - BAR_INSET;
            let bar = ruler::scale_bar(viewport, right, CAPTION_HEIGHT / 2.0);
            self.draw(&bar, (0.0, 0.0));
        }
    }

    /// The header of each panel: its locus and its scale bar.
    fn headers(&mut self, session: &Session, y: f32) {
        let tokens = self.colors.0;
        for panel in 0..self.panels.len() {
            let (left, width) = self.panels[panel];
            let at = (left + LOCUS_INSET, y + HEADER_HEIGHT / 2.0);
            let locus = session.locus_at(panel);
            let (size, color) = (CAPTION_SIZE, tokens.text);
            self.svg
                .text(at, &locus, size, Face::Mono, color, Align::Start);
            if session.figure.scale {
                let (_, viewport) = session.view(panel);
                let bar = ruler::scale_bar(viewport, width - BAR_INSET, HEADER_HEIGHT / 2.0);
                self.draw_in(panel, (y, HEADER_HEIGHT), &[(&bar, y)]);
            }
        }
        self.gaps(y, HEADER_HEIGHT);
    }

    /// The ruler of each panel. For the whole genome, the names of the chromosomes.
    fn rulers(&mut self, session: &Session, y: f32) {
        let muted = self.colors.0.muted;
        let contigs = session.reference.assembly.contigs();
        for panel in 0..self.panels.len() {
            let (contig, viewport) = session.view(panel);
            if session.genome {
                let (left, width) = self.panels[panel];
                if let Some(name) = whole::name(&contigs[contig].name, width) {
                    let at = (left + width / 2.0, y + RULER_HEIGHT / 2.0);
                    self.svg
                        .text(at, &name, CAPTION_SIZE, Face::Ui, muted, Align::Center);
                }
            } else {
                let ticks = ruler::scene(viewport, RULER_HEIGHT);
                self.draw_in(panel, (y, RULER_HEIGHT), &[(&ticks, y)]);
            }
        }
        self.gaps(y, RULER_HEIGHT);
    }

    /// The bands of the regions in view, when the figure shows them. Gives the height they take.
    fn bands(&mut self, session: &Session, y: f32) -> f32 {
        let Some(rows) = band_rows(session) else {
            return 0.0;
        };
        let height = rows as f32 * bands::ROW_HEIGHT;
        for panel in 0..self.panels.len() {
            let (primitives, _) = panel_bands(session, panel);
            self.draw_in(panel, (y, height), &[(&primitives, y)]);
        }
        self.gaps(y, height);
        self.hairline(y + height);
        height + 1.0
    }

    fn head(&mut self, session: &Session) -> f32 {
        self.caption(session);
        let mut y = CAPTION_HEIGHT;
        self.hairline(y);
        y += 1.0;
        if !one_caption(session) {
            self.headers(session, y);
            y += HEADER_HEIGHT;
            self.hairline(y);
            y += 1.0;
        }
        self.rulers(session, y);
        y += RULER_HEIGHT;
        self.hairline(y);
        y += 1.0;
        y + self.bands(session, y)
    }

    fn lane(&mut self, session: &Session, track: &Track, y: f32, padding: f32) -> f32 {
        let (tokens, data) = self.colors;
        let (shapes, tallest) = shapes(session, track, true);
        let height = track.height.unwrap_or(tallest.height);
        let top = y + padding;
        for (panel, shape) in shapes.into_iter().enumerate() {
            let (x, column) = self.panels[panel];
            self.svg.clip((x, y, column, height + 2.0 * padding));
            match body(session, track, panel, shape, true) {
                Body::Hint(words) => self.svg.text(
                    (x + column / 2.0, top + EMPTY_HEIGHT / 2.0),
                    &words,
                    HINT_SIZE,
                    Face::Ui,
                    tokens.muted,
                    Align::Center,
                ),
                Body::Space(_) => {}
                Body::Scene(primitives, _) => self.draw(&primitives, (x, top)),
                Body::Reads { coverage, reads } => {
                    self.draw(&coverage, (x, top));
                    if let Some((primitives, _)) = reads {
                        let below = top + alignments::COVERAGE_HEIGHT + 1.0;
                        self.draw(&primitives, (x, below));
                    }
                }
            }
            self.svg.unclip();
        }
        self.gaps(top, height);
        let members = members(session, track);
        let names: Vec<(&str, Color)> = if members.is_empty() {
            vec![(track.name.as_str(), tokens.muted)]
        } else {
            std::iter::once(track)
                .chain(members)
                .map(|track| (track.name.as_str(), data.hues[track.hue as usize]))
                .collect()
        };
        self.chip((0.0, y), &names);
        let bottom = y + height + 2.0 * padding;
        let hidden = tallest.hidden(height);
        if hidden > 0 {
            let words = if hidden == 1 {
                "+1 row".to_owned()
            } else {
                format!("+{hidden} rows")
            };
            let width = svg::width(&words, LABEL_SIZE, Face::Ui) + 8.0;
            let at = (
                self.width - CHIP_EDGE - width,
                bottom - 1.0 - LABEL_SIZE * LINE_HEIGHT,
            );
            self.chip(at, &[(&words, tokens.muted)]);
        }
        self.hairline(bottom);
        bottom + 1.0
    }
}

impl Genie {
    pub(super) fn figure(&self, session: &Session) -> String {
        let theme = self.theme();
        let mut left = 0.0;
        let panels = session
            .widths(self.width)
            .into_iter()
            .map(|width| {
                let panel = (left, width);
                left += width + PANEL_GAP;
                panel
            })
            .collect();
        let mut page = Page {
            svg: Svg::new(self.width),
            width: self.width,
            panels,
            colors: (tokens(&theme), data(&theme)),
        };
        let mut y = page.head(session);
        let padding = self.settings.density.padding();
        let frames = session.reading.frames();
        let block = BASE_HEIGHT + frames.len() as f32 * translation::ROW_HEIGHT;
        for panel in 0..page.panels.len() {
            let (contig, viewport) = session.view(panel);
            let chunks = || self.tiles.chunks(contig, viewport.visible());
            let bases = sequence::scene(viewport, chunks(), BASE_HEIGHT, session.reading.reverse);
            let codons = if frames.is_empty() {
                Vec::new()
            } else {
                translation::scene(viewport, chunks(), &frames)
            };
            let parts = [
                (&bases[..], y + padding),
                (&codons[..], y + padding + BASE_HEIGHT),
            ];
            page.draw_in(panel, (y + padding, block), &parts);
        }
        page.gaps(y + padding, block);
        let muted = page.colors.0.muted;
        page.chip((0.0, y), &[("Sequence", muted)]);
        y += block + 2.0 * padding;
        page.hairline(y);
        y += 1.0;
        for track in session
            .tracks
            .iter()
            .filter(|track| track.over.is_none() && !track.hidden)
        {
            y = page.lane(session, track, y, padding);
        }
        page.svg.finish(y, theme.base().background_color)
    }
}
