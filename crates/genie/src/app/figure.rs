use genie_render::scene::Align;
use genie_render::{alignments, ruler, sequence, translation};
use iced::Color;
use iced::theme::Base;

use crate::scene::CHIP;
use crate::svg::{self, Face, Svg};
use crate::theme::{DataColors, Tokens, data, tokens};

use super::export::{CAPTION_HEIGHT, CAPTION_SIZE};
use super::view::{
    BASE_HEIGHT, Body, EMPTY_HEIGHT, LABEL_SIZE, RULER_HEIGHT, body, members, shape,
};
use super::{Genie, Session};

const LINE_HEIGHT: f32 = 1.3;
const HINT_SIZE: f32 = 11.0;
const GAP: f32 = 6.0;

struct Page<'a> {
    svg: Svg,
    width: f32,
    colors: (&'a Tokens, &'a DataColors),
}

impl Page<'_> {
    fn hairline(&mut self, y: f32) {
        self.svg.rect(0.0, y, self.width, 1.0, self.colors.0.line);
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

    fn draw(&mut self, primitives: &[genie_render::scene::Primitive], top: f32) {
        svg::draw(&mut self.svg, primitives, (0.0, top), self.colors);
    }
}

impl Genie {
    pub(super) fn figure(&self, session: &Session) -> String {
        let theme = self.theme();
        let (tokens, data) = (tokens(&theme), data(&theme));
        let mut page = Page {
            svg: Svg::new(self.width),
            width: self.width,
            colors: (tokens, data),
        };
        let viewport = session.viewport;
        let center = CAPTION_HEIGHT / 2.0;
        let name = session.reference.name.as_str();
        let locus = session.locus();
        let caption = [
            (name, Face::Ui, tokens.text),
            ("·", Face::Ui, tokens.muted),
            (locus.as_str(), Face::Mono, tokens.muted),
        ];
        page.svg.spans((8.0, center), CAPTION_SIZE, &caption, GAP);
        let mut y = CAPTION_HEIGHT;
        page.hairline(y);
        y += 1.0;
        page.draw(&ruler::scene(viewport, RULER_HEIGHT), y);
        y += RULER_HEIGHT;
        page.hairline(y);
        y += 1.0;
        let padding = self.settings.density.padding();
        let visible = viewport.visible();
        let chunks = || self.tiles.chunks(session.contig, visible);
        let bases = sequence::scene(viewport, chunks(), BASE_HEIGHT, session.reading.reverse);
        page.draw(&bases, y + padding);
        let mut block = BASE_HEIGHT;
        let frames = session.reading.frames();
        if !frames.is_empty() {
            let codons = translation::scene(viewport, chunks(), &frames);
            page.draw(&codons, y + padding + BASE_HEIGHT);
            block += frames.len() as f32 * translation::ROW_HEIGHT;
        }
        page.chip((0.0, y), &[("Sequence", tokens.muted)]);
        y += block + 2.0 * padding;
        page.hairline(y);
        y += 1.0;
        for track in session.tracks.iter().filter(|track| track.over.is_none()) {
            let shape = shape(session, track, 0, true);
            let height = track.height.unwrap_or(shape.height);
            let top = y + padding;
            page.svg.clip((0.0, y, self.width, height + 2.0 * padding));
            match body(session, track, 0, shape, true) {
                Body::Hint(words) => page.svg.text(
                    (self.width / 2.0, top + EMPTY_HEIGHT / 2.0),
                    &words,
                    HINT_SIZE,
                    Face::Ui,
                    tokens.muted,
                    Align::Center,
                ),
                Body::Space(_) => {}
                Body::Scene(primitives, _) => page.draw(&primitives, top),
                Body::Reads { coverage, reads } => {
                    page.draw(&coverage, top);
                    if let Some((primitives, _)) = reads {
                        page.draw(&primitives, top + alignments::COVERAGE_HEIGHT + 1.0);
                    }
                }
            }
            page.svg.unclip();
            let members = members(session, track);
            let names: Vec<(&str, Color)> = if members.is_empty() {
                vec![(track.name.as_str(), tokens.muted)]
            } else {
                std::iter::once(track)
                    .chain(members)
                    .map(|track| (track.name.as_str(), data.hues[track.hue as usize]))
                    .collect()
            };
            page.chip((0.0, y), &names);
            let bottom = y + height + 2.0 * padding;
            let hidden = shape.hidden(height);
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
                page.chip(at, &[(&words, tokens.muted)]);
            }
            y = bottom;
            page.hairline(y);
            y += 1.0;
        }
        page.svg.finish(y, theme.base().background_color)
    }
}
