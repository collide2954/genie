use genie_render::scene::{Align, Primitive};
use genie_render::{alignments, ruler, sequence, translation};
use iced::Color;
use iced::theme::Base;

use crate::scene::CHIP;
use crate::svg::{self, Face, Svg};
use crate::theme::{DataColors, Tokens, data, tokens};
use crate::track::Track;

use super::export::{CAPTION_HEIGHT, CAPTION_SIZE};
use super::view::{
    BASE_HEIGHT, Body, EMPTY_HEIGHT, HEADER_HEIGHT, LABEL_SIZE, RULER_HEIGHT, body, members, shapes,
};
use super::{Genie, PANEL_GAP, Session, panel_width};

const LINE_HEIGHT: f32 = 1.3;
const HINT_SIZE: f32 = 11.0;
const GAP: f32 = 6.0;
const LOCUS_INSET: f32 = 10.0;

struct Page<'a> {
    svg: Svg,
    width: f32,
    count: usize,
    colors: (&'a Tokens, &'a DataColors),
}

impl Page<'_> {
    fn hairline(&mut self, y: f32) {
        self.svg.rect(0.0, y, self.width, 1.0, self.colors.0.line);
    }

    fn column(&self) -> f32 {
        panel_width(self.width, self.count)
    }

    fn left(&self, panel: usize) -> f32 {
        panel as f32 * (self.column() + PANEL_GAP)
    }

    fn gaps(&mut self, y: f32, height: f32) {
        for panel in 1..self.count {
            let x = self.left(panel) - PANEL_GAP;
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
        let x = self.left(panel);
        self.svg.clip((x, y, self.column(), height));
        for (primitives, top) in parts {
            self.draw(primitives, (x, *top));
        }
        self.svg.unclip();
    }
}

impl Page<'_> {
    fn head(&mut self, session: &Session) -> f32 {
        let tokens = self.colors.0;
        let locus = session.locus();
        let mut caption = vec![(session.reference.name.as_str(), Face::Ui, tokens.text)];
        if self.count == 1 {
            caption.push(("·", Face::Ui, tokens.muted));
            caption.push((locus.as_str(), Face::Mono, tokens.muted));
        }
        self.svg
            .spans((8.0, CAPTION_HEIGHT / 2.0), CAPTION_SIZE, &caption, GAP);
        let mut y = CAPTION_HEIGHT;
        self.hairline(y);
        y += 1.0;
        if self.count > 1 {
            for panel in 0..self.count {
                let at = (self.left(panel) + LOCUS_INSET, y + HEADER_HEIGHT / 2.0);
                let locus = session.locus_at(panel);
                let (size, color) = (CAPTION_SIZE, tokens.text);
                self.svg
                    .text(at, &locus, size, Face::Mono, color, Align::Start);
            }
            self.gaps(y, HEADER_HEIGHT);
            y += HEADER_HEIGHT;
            self.hairline(y);
            y += 1.0;
        }
        for panel in 0..self.count {
            let (_, viewport) = session.view(panel);
            let ticks = ruler::scene(viewport, RULER_HEIGHT);
            self.draw_in(panel, (y, RULER_HEIGHT), &[(&ticks, y)]);
        }
        self.gaps(y, RULER_HEIGHT);
        y += RULER_HEIGHT;
        self.hairline(y);
        y + 1.0
    }

    fn lane(&mut self, session: &Session, track: &Track, y: f32, padding: f32) -> f32 {
        let (tokens, data) = self.colors;
        let (shapes, tallest) = shapes(session, track, true);
        let height = track.height.unwrap_or(tallest.height);
        let top = y + padding;
        for (panel, shape) in shapes.into_iter().enumerate() {
            let (x, column) = (self.left(panel), self.column());
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
                self.width - 4.0 - width,
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
        let mut page = Page {
            svg: Svg::new(self.width),
            width: self.width,
            count: session.count(),
            colors: (tokens(&theme), data(&theme)),
        };
        let mut y = page.head(session);
        let padding = self.settings.density.padding();
        let frames = session.reading.frames();
        let block = BASE_HEIGHT + frames.len() as f32 * translation::ROW_HEIGHT;
        for panel in 0..page.count {
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
        for track in session.tracks.iter().filter(|track| track.over.is_none()) {
            y = page.lane(session, track, y, padding);
        }
        page.svg.finish(y, theme.base().background_color)
    }
}
