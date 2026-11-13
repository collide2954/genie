use std::borrow::Cow;

use genie_core::{Feature, Region, Strand, Viewport};

use crate::scene::{Align, Font, Hue, Paint, Primitive};

pub const LIMIT: usize = 10;

const LABEL_SIZE: f32 = 10.0;
const LABEL_GAP: f32 = 6.0;
const GLYPH_WIDTH: f32 = 0.6;
const CHEVRON_SIZE: f32 = 2.5;
const CHEVRON_SPACING: f32 = 24.0;
const CHEVRON_MIN: f32 = 12.0;
const REACH: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Expanded,
    Squished,
    Collapsed,
}

struct Shape {
    row: f32,
    tall: (f32, f32),
    short: (f32, f32),
    line: f32,
    marks: bool,
}

const FULL: Shape = Shape {
    row: 20.0,
    tall: (1.0, 9.0),
    short: (3.0, 5.0),
    line: 5.0,
    marks: true,
};

const SQUISHED: Shape = Shape {
    row: 6.0,
    tall: (0.0, 5.0),
    short: (1.0, 3.0),
    line: 2.0,
    marks: false,
};

impl Mode {
    pub fn fit(rows: usize) -> Self {
        if rows <= LIMIT {
            Self::Expanded
        } else {
            Self::Squished
        }
    }

    pub fn row_height(self) -> f32 {
        self.shape().row
    }

    fn shape(self) -> &'static Shape {
        match self {
            Self::Expanded | Self::Collapsed => &FULL,
            Self::Squished => &SQUISHED,
        }
    }
}

pub fn hit(
    viewport: Viewport,
    features: &[Feature],
    rows: &[usize],
    mode: Mode,
    x: f32,
    y: f32,
) -> Option<usize> {
    if y < 0.0 {
        return None;
    }
    let row = (y / mode.shape().row) as usize;
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    features
        .iter()
        .zip(rows)
        .enumerate()
        .filter(|(_, (_, own))| match mode {
            Mode::Collapsed => row == 0,
            Mode::Expanded | Mode::Squished => **own == row,
        })
        .map(|(index, (feature, _))| {
            let span = feature.span();
            let distance = (px(span.start()) - x).max(x - px(span.end())).max(0.0);
            (index, distance)
        })
        .filter(|(_, distance)| *distance <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

pub fn scene(
    viewport: Viewport,
    features: &[Feature],
    rows: &[usize],
    mode: Mode,
    hue: Hue,
    colors: bool,
) -> Vec<Primitive> {
    let shape = mode.shape();
    let visible = viewport.visible();
    let width = viewport.width() as f32;
    let px = |bp: u64| viewport.to_px(bp as f64) as f32;
    let bar = |region: Region, top: f32, (offset, height): (f32, f32), paint: Paint| {
        let left = px(region.start()).max(-1.0);
        let right = px(region.end()).min(width + 1.0);
        Primitive::Rect {
            x: left,
            y: top + offset,
            width: (right - left).max(1.0),
            height,
            paint,
        }
    };
    let mut primitives = Vec::new();
    let mut labels = Labels::default();
    let mut narrow = Columns::default();
    for (feature, &row) in features.iter().zip(rows) {
        let span = feature.span();
        if span.start() > visible.end() || span.end() < visible.start() {
            continue;
        }
        let row = if mode == Mode::Collapsed { 0 } else { row };
        let extent = (px(span.start()), px(span.end()));
        if narrow.taken(row, extent) {
            continue;
        }
        let top = row as f32 * shape.row;
        let paint = feature
            .color()
            .filter(|_| colors)
            .map_or(Paint::Hue(hue), Paint::Rgb);
        let blocks = feature.blocks();
        for pair in blocks.windows(2) {
            let (left, right) = (px(pair[0].end()), px(pair[1].start()));
            if right <= left {
                continue;
            }
            if let Some(intron) = Region::new(pair[0].end(), pair[1].start()) {
                primitives.push(bar(intron, top, (shape.line, 1.0), paint));
            }
            if shape.marks && feature.strand() != Strand::Unknown {
                let forward = feature.strand() == Strand::Forward;
                chevrons(
                    &mut primitives,
                    (left, right),
                    width,
                    top + shape.line + 0.5,
                    forward,
                    paint,
                );
            }
        }
        for block in blocks {
            match feature
                .coding()
                .and_then(|coding| coding.intersection(*block))
            {
                Some(coding) => {
                    if let Some(before) =
                        Region::new(block.start(), coding.start()).filter(|part| !part.is_empty())
                    {
                        primitives.push(bar(before, top, shape.short, paint));
                    }
                    primitives.push(bar(coding, top, shape.tall, paint));
                    if let Some(after) =
                        Region::new(coding.end(), block.end()).filter(|part| !part.is_empty())
                    {
                        primitives.push(bar(after, top, shape.short, paint));
                    }
                }
                None => primitives.push(bar(*block, top, shape.short, paint)),
            }
        }
        if shape.marks && !feature.name().is_empty() {
            let y = top + shape.row - LABEL_SIZE / 2.0;
            labels.place(feature.name(), row, extent, (width, y));
        }
    }
    primitives.extend(labels.texts);
    primitives
}

#[derive(Default)]
struct Columns(Vec<i64>);

impl Columns {
    fn taken(&mut self, row: usize, (left, right): (f32, f32)) -> bool {
        if right - left >= 1.0 {
            return false;
        }
        if self.0.len() <= row {
            self.0.resize(row + 1, i64::MIN);
        }
        let column = left.floor() as i64;
        let taken = self.0[row] == column;
        self.0[row] = column;
        taken
    }
}

#[derive(Default)]
struct Labels {
    edges: Vec<f32>,
    texts: Vec<Primitive>,
}

impl Labels {
    fn place(&mut self, name: &str, row: usize, (left, right): (f32, f32), (width, y): (f32, f32)) {
        let half = name.chars().count() as f32 * LABEL_SIZE * GLYPH_WIDTH / 2.0;
        let center = f32::midpoint(left.max(0.0), right.min(width));
        let center = if width > half * 2.0 {
            center.clamp(half, width - half)
        } else {
            center
        };
        if self.edges.len() <= row {
            self.edges.resize(row + 1, f32::NEG_INFINITY);
        }
        if center - half >= self.edges[row] + LABEL_GAP {
            self.edges[row] = center + half;
            self.texts.push(Primitive::Text {
                x: center,
                y,
                content: Cow::Owned(name.to_owned()),
                size: LABEL_SIZE,
                paint: Paint::Text,
                font: Font::Ui,
                align: Align::Center,
            });
        }
    }
}

fn chevrons(
    primitives: &mut Vec<Primitive>,
    (left, right): (f32, f32),
    width: f32,
    y: f32,
    forward: bool,
    paint: Paint,
) {
    let length = right - left;
    if length < CHEVRON_MIN {
        return;
    }
    let count = (length / CHEVRON_SPACING).floor().max(1.0);
    let step = length / count;
    let first = ((-left / step - 0.5).ceil()).max(0.0);
    let last = ((width - left) / step - 0.5).floor().min(count - 1.0);
    let mut index = first;
    while index <= last {
        primitives.push(Primitive::Chevron {
            x: left + step * (index + 0.5),
            y,
            size: CHEVRON_SIZE,
            forward,
            paint,
        });
        index += 1.0;
    }
}

#[cfg(test)]
mod tests {
    use genie_core::{Feature, Region, Strand, Viewport};

    use super::{LIMIT, Mode, hit, scene};
    use crate::scene::{Hue, Paint, Primitive};

    fn region(start: u64, end: u64) -> Region {
        Region::new(start, end).unwrap()
    }

    fn view() -> Viewport {
        Viewport::new(1_000_000, 100.0).show(region(0, 100))
    }

    fn named(name: &str, start: u64, end: u64) -> Feature {
        Feature::new(name, region(start, end), Strand::Forward)
    }

    fn transcript(strand: Strand) -> Feature {
        Feature::new("ABC", region(10, 90), strand)
            .with_blocks(vec![region(10, 30), region(70, 90)])
            .with_coding(Some(region(20, 80)))
    }

    fn draw(features: &[Feature], rows: &[usize], mode: Mode) -> Vec<Primitive> {
        scene(view(), features, rows, mode, Hue::Green, true)
    }

    #[test]
    fn narrow_features_share_pixel_columns() {
        let features: Vec<Feature> = (0..10_000)
            .map(|start| Feature::new("", region(start, start + 1), Strand::Unknown))
            .collect();
        let wide = Viewport::new(1_000_000, 100.0).show(region(0, 10_000));
        for (mode, rows) in [
            (Mode::Collapsed, vec![0; 10_000]),
            (Mode::Squished, (0..10_000).map(|index| index % 3).collect()),
        ] {
            let found = scene(wide, &features, &rows, mode, Hue::Green, true);
            assert!(found.len() <= 3 * 101, "{mode:?}: {}", found.len());
        }
        let wider = [named("a", 0, 10_000)];
        assert_eq!(
            scene(wide, &wider, &[0], Mode::Squished, Hue::Green, true).len(),
            1
        );
    }

    #[test]
    fn file_colors_paint_their_features() {
        let colored =
            Feature::new("a", region(10, 20), Strand::Unknown).with_color(Some([1, 2, 3]));
        let plain = Feature::new("b", region(30, 40), Strand::Unknown);
        let paints = |colors| -> Vec<Paint> {
            scene(
                view(),
                &[colored.clone(), plain.clone()],
                &[0, 0],
                Mode::Squished,
                Hue::Blue,
                colors,
            )
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect { paint, .. } => Some(*paint),
                _ => None,
            })
            .collect()
        };
        assert_eq!(paints(true), [Paint::Rgb([1, 2, 3]), Paint::Hue(Hue::Blue)]);
        assert_eq!(
            paints(false),
            [Paint::Hue(Hue::Blue), Paint::Hue(Hue::Blue)]
        );
    }

    fn rects(primitives: &[Primitive]) -> Vec<[f32; 4]> {
        let mut rects: Vec<[f32; 4]> = primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some([*x, *y, *width, *height]),
                _ => None,
            })
            .collect();
        rects.sort_by(|a, b| a.partial_cmp(b).unwrap());
        rects
    }

    fn texts(primitives: &[Primitive]) -> Vec<(String, f32, f32)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Text { content, x, y, .. } => Some((content.to_string(), *x, *y)),
                _ => None,
            })
            .collect()
    }

    fn chevrons(primitives: &[Primitive]) -> Vec<(f32, f32, bool)> {
        primitives
            .iter()
            .filter_map(|primitive| match primitive {
                Primitive::Chevron { x, y, forward, .. } => Some((*x, *y, *forward)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hits_the_feature_under_the_pointer() {
        let features = [named("a", 10, 20), named("b", 10, 20), named("c", 50, 60)];
        let rows = [0, 1, 0];
        let at = |mode, x, y| hit(view(), &features, &rows, mode, x, y);
        assert_eq!(at(Mode::Expanded, 15.0, 5.0), Some(0));
        assert_eq!(at(Mode::Expanded, 15.0, 25.0), Some(1));
        assert_eq!(at(Mode::Expanded, 55.0, 5.0), Some(2));
        assert_eq!(at(Mode::Expanded, 35.0, 5.0), None);
        assert_eq!(at(Mode::Squished, 15.0, 7.0), Some(1));
        assert_eq!(at(Mode::Collapsed, 55.0, 5.0), Some(2));
        assert_eq!(at(Mode::Collapsed, 15.0, 25.0), None);
    }

    #[test]
    fn hits_small_features_near_the_pointer() {
        let wide = Viewport::new(1_000_000, 100.0).show(region(0, 10_000));
        let features = [named("a", 1_000, 1_001)];
        assert_eq!(
            hit(wide, &features, &[0], Mode::Expanded, 11.5, 5.0),
            Some(0)
        );
        assert_eq!(hit(wide, &features, &[0], Mode::Expanded, 14.0, 5.0), None);
    }

    #[test]
    fn fit_squishes_rows_past_the_limit() {
        assert_eq!(Mode::fit(LIMIT), Mode::Expanded);
        assert_eq!(Mode::fit(LIMIT + 1), Mode::Squished);
    }

    #[test]
    fn draws_coding_parts_tall_and_untranslated_parts_short() {
        let primitives = draw(&[transcript(Strand::Forward)], &[0], Mode::Expanded);
        assert_eq!(
            rects(&primitives),
            [
                [10.0, 3.0, 10.0, 5.0],
                [20.0, 1.0, 10.0, 9.0],
                [30.0, 5.0, 40.0, 1.0],
                [70.0, 1.0, 10.0, 9.0],
                [80.0, 3.0, 10.0, 5.0],
            ]
        );
    }

    #[test]
    fn paints_glyphs_with_the_track_hue_and_labels_as_text() {
        for primitive in draw(&[transcript(Strand::Forward)], &[0], Mode::Expanded) {
            let paint = match primitive {
                Primitive::Rect { paint, .. } | Primitive::Chevron { paint, .. } => paint,
                Primitive::Text { paint, .. } => {
                    assert_eq!(paint, Paint::Text);
                    continue;
                }
                Primitive::Label { .. } | Primitive::Arc { .. } => {
                    panic!("features have no range labels or arcs")
                }
            };
            assert_eq!(paint, Paint::Hue(Hue::Green));
        }
    }

    #[test]
    fn writes_labels_below_features() {
        let primitives = draw(&[transcript(Strand::Forward)], &[0], Mode::Expanded);
        assert_eq!(texts(&primitives), [("ABC".to_owned(), 50.0, 15.0)]);
    }

    #[test]
    fn stacks_rows_by_mode() {
        let features = [named("a", 10, 20), named("b", 10, 20)];
        let tops = |mode| -> Vec<f32> {
            rects(&draw(&features, &[0, 1], mode))
                .iter()
                .map(|rect| rect[1])
                .collect()
        };
        assert_eq!(tops(Mode::Expanded), [1.0, 21.0]);
        assert_eq!(tops(Mode::Squished), [0.0, 6.0]);
        assert_eq!(tops(Mode::Collapsed), [1.0, 1.0]);
    }

    #[test]
    fn squished_rows_have_no_labels_or_chevrons() {
        let primitives = draw(&[transcript(Strand::Forward)], &[0], Mode::Squished);
        assert_eq!(texts(&primitives), []);
        assert_eq!(chevrons(&primitives), []);
    }

    #[test]
    fn skips_labels_that_would_overlap() {
        let features = [
            named("first", 10, 12),
            named("second", 14, 16),
            named("third", 80, 82),
        ];
        let primitives = draw(&features, &[0, 0, 0], Mode::Expanded);
        let names: Vec<String> = texts(&primitives).into_iter().map(|text| text.0).collect();
        assert_eq!(names, ["first", "third"]);
    }

    #[test]
    fn points_chevrons_on_introns_with_the_strand() {
        let forward = draw(&[transcript(Strand::Forward)], &[0], Mode::Expanded);
        assert_eq!(chevrons(&forward), [(50.0, 5.5, true)]);
        let reverse = draw(&[transcript(Strand::Reverse)], &[0], Mode::Expanded);
        assert_eq!(chevrons(&reverse), [(50.0, 5.5, false)]);
        let unknown = draw(&[transcript(Strand::Unknown)], &[0], Mode::Expanded);
        assert_eq!(chevrons(&unknown), []);
    }

    #[test]
    fn skips_features_outside_the_view() {
        let features = [named("a", 10, 20), named("b", 500, 600)];
        assert_eq!(rects(&draw(&features, &[0, 0], Mode::Expanded)).len(), 1);
    }

    #[test]
    fn keeps_small_blocks_one_pixel_wide() {
        let wide = Viewport::new(1_000_000, 100.0).show(region(0, 10_000));
        let features = [named("", 10, 20)];
        let primitives = scene(wide, &features, &[0], Mode::Expanded, Hue::Blue, true);
        assert_eq!(rects(&primitives), [[0.1, 1.0, 1.0, 9.0]]);
    }
}
