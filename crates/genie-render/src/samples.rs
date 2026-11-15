//! The rows of tracks with a row for each sample, such as genotypes and copy number segments.
//! A sample table can put the rows in groups, sort them and color them.

use std::borrow::Cow;

use genie_core::grouped;
use genie_core::samples::{Arrangement, Tint};

use crate::scene::{Align, Font, Paint, Primitive};

/// The height of the band above each group, with the name of the group.
pub const GROUP_GAP: f32 = 14.0;
/// The width of the strip at the left of each row that shows the color of its samples.
pub const STRIP: f32 = 4.0;
const GROUP_SIZE: f32 = 10.0;
const GROUP_INSET: f32 = 4.0;

/// The rows of the samples of a track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rows {
    /// The samples of each row, by their number in the file.
    pub members: Vec<Vec<usize>>,
    /// The first row of each group and its label.
    pub groups: Vec<(usize, String)>,
    /// The paint of the strip of each row, or empty when no field colors the rows.
    pub strip: Vec<Paint>,
    /// The row of each sample, by its number in the file.
    pub place: Vec<usize>,
}

impl Rows {
    /// Rows of at most `size` samples, in the order and the groups of an arrangement. All the
    /// samples of a row are in the same group.
    pub fn new(arrangement: &Arrangement, size: usize) -> Self {
        let size = size.max(1);
        let total = arrangement.order.len();
        let mut parts: Vec<(usize, usize, Option<String>)> = arrangement
            .groups
            .iter()
            .map(|group| {
                let end = (group.first + group.size).min(total);
                (
                    group.first.min(end),
                    end,
                    Some(label(&group.name, group.size)),
                )
            })
            .collect();
        if parts.is_empty() {
            parts.push((0, total, None));
        }
        let mut rows = Self {
            place: vec![0; total],
            ..Self::default()
        };
        for (first, end, name) in parts {
            if let Some(name) = name {
                rows.groups.push((rows.members.len(), name));
            }
            for part in arrangement.order[first..end].chunks(size) {
                for &sample in part {
                    if let Some(place) = rows.place.get_mut(sample) {
                        *place = rows.members.len();
                    }
                }
                rows.members.push(part.to_vec());
            }
        }
        if !arrangement.tints.is_empty() {
            rows.strip = rows
                .members
                .iter()
                .map(|members| paint(shared(members, &arrangement.tints)))
                .collect();
        }
        rows
    }

    /// Rows of `size` samples in the order of the file.
    pub fn plain(samples: usize, size: usize) -> Self {
        Self::new(&Arrangement::plain(samples), size)
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The height of the bands of the groups above a row and of its own group.
    fn bands(&self, row: usize) -> f32 {
        self.groups.partition_point(|(first, _)| *first <= row) as f32 * GROUP_GAP
    }

    /// The top of a row.
    pub fn top(&self, row: usize, height: f32) -> f32 {
        row as f32 * height + self.bands(row)
    }

    /// The height of all the rows and bands.
    pub fn height(&self, height: f32) -> f32 {
        self.len() as f32 * height + self.groups.len() as f32 * GROUP_GAP
    }

    /// The row at a height, or `None` in a band or after the last row.
    pub fn at(&self, y: f32, height: f32) -> Option<usize> {
        if y < 0.0 || height <= 0.0 {
            return None;
        }
        let row = if self.groups.is_empty() {
            (y / height) as usize
        } else {
            let top = |index: usize| {
                self.groups[index].0 as f32 * height + (index + 1) as f32 * GROUP_GAP
            };
            let index = (0..self.groups.len())
                .rev()
                .find(|&index| y >= top(index))?;
            let row = self.groups[index].0 + ((y - top(index)) / height) as usize;
            let end = self
                .groups
                .get(index + 1)
                .map_or(usize::MAX, |(next, _)| *next);
            if row >= end {
                return None;
            }
            row
        };
        (row < self.len()).then_some(row)
    }

    /// The strips of the rows and the names of the groups. `gap` is the space under each row.
    pub fn marks(&self, height: f32, gap: f32) -> Vec<Primitive> {
        let mut marks: Vec<Primitive> = self
            .strip
            .iter()
            .enumerate()
            .map(|(row, paint)| Primitive::Rect {
                x: 0.0,
                y: self.top(row, height),
                width: STRIP,
                height: (height - gap).max(1.0),
                paint: *paint,
            })
            .collect();
        marks.extend(
            self.groups
                .iter()
                .enumerate()
                .map(|(index, (first, name))| Primitive::Text {
                    x: GROUP_INSET,
                    y: *first as f32 * height + index as f32 * GROUP_GAP + GROUP_GAP / 2.0,
                    content: Cow::Owned(name.clone()),
                    size: GROUP_SIZE,
                    paint: Paint::Muted,
                    font: Font::Ui,
                    align: Align::Start,
                }),
        );
        marks
    }
}

/// The label of a group: its name and its number of samples.
pub fn label(name: &str, size: usize) -> String {
    if size == 1 {
        format!("{name} · 1 sample")
    } else {
        format!("{name} · {} samples", grouped(size as u64))
    }
}

/// The paint of the strip of a sample.
pub fn paint(tint: Tint) -> Paint {
    match tint {
        Tint::Value(place) => Paint::Category(place),
        Tint::Level(level) => Paint::Level(level),
        Tint::Other => Paint::Line,
    }
}

/// The tint of a row of samples: the mean level of its numbers, or else its most common value.
/// Of two values that are as common, the more common value of the field wins.
fn shared(members: &[usize], tints: &[Tint]) -> Tint {
    let found: Vec<Tint> = members
        .iter()
        .filter_map(|&sample| tints.get(sample).copied())
        .collect();
    let levels: Vec<u32> = found
        .iter()
        .filter_map(|tint| match tint {
            Tint::Level(level) => Some(u32::from(*level)),
            _ => None,
        })
        .collect();
    if !levels.is_empty() {
        let mean = levels.iter().sum::<u32>() / levels.len() as u32;
        return Tint::Level(u8::try_from(mean).unwrap_or(100));
    }
    let mut counts = [0_usize; 256];
    for tint in &found {
        if let Tint::Value(place) = tint {
            counts[usize::from(*place)] += 1;
        }
    }
    let best = counts.iter().copied().max().unwrap_or(0);
    if best == 0 {
        return Tint::Other;
    }
    counts
        .iter()
        .position(|count| *count == best)
        .and_then(|place| u8::try_from(place).ok())
        .map_or(Tint::Other, Tint::Value)
}

#[cfg(test)]
mod tests {
    use genie_core::samples::{Arrangement, Group, Tint};

    use super::{GROUP_GAP, Rows, STRIP, label, paint};
    use crate::scene::{Paint, Primitive};

    fn group(name: &str, first: usize, size: usize) -> Group {
        Group {
            name: name.to_owned(),
            first,
            size,
        }
    }

    fn by_population() -> Arrangement {
        Arrangement {
            order: vec![1, 4, 5, 0, 2, 3, 6],
            groups: vec![
                group("AFR", 0, 2),
                group("EAS", 2, 1),
                group("EUR", 3, 2),
                group("No pop", 5, 1),
                group("Not in the sample table", 6, 1),
            ],
            tints: Vec::new(),
        }
    }

    #[test]
    fn rows_follow_the_order_and_the_groups_of_an_arrangement() {
        let rows = Rows::new(&by_population(), 1);
        assert_eq!(
            rows.members,
            [
                vec![1],
                vec![4],
                vec![5],
                vec![0],
                vec![2],
                vec![3],
                vec![6]
            ]
        );
        assert_eq!(rows.place, [3, 0, 4, 5, 1, 2, 6]);
        let firsts: Vec<usize> = rows.groups.iter().map(|(first, _)| *first).collect();
        assert_eq!(firsts, [0, 2, 3, 5, 6]);
        assert_eq!(rows.groups[0].1, "AFR · 2 samples");
        assert_eq!(rows.groups[1].1, "EAS · 1 sample");
        assert_eq!(rows.strip, []);
        let pairs = Rows::new(&by_population(), 2);
        assert_eq!(
            pairs.members,
            [vec![1, 4], vec![5], vec![0, 2], vec![3], vec![6]]
        );
        let firsts: Vec<usize> = pairs.groups.iter().map(|(first, _)| *first).collect();
        assert_eq!(firsts, [0, 1, 2, 3, 4]);
        let plain = Rows::plain(5, 2);
        assert_eq!(plain.members, [vec![0, 1], vec![2, 3], vec![4]]);
        assert_eq!((plain.groups.len(), plain.place), (0, vec![0, 0, 1, 1, 2]));
    }

    #[test]
    fn rows_know_their_tops_and_the_row_at_a_height() {
        let two = Arrangement {
            order: vec![0, 1, 2, 3],
            groups: vec![group("A", 0, 2), group("B", 2, 2)],
            tints: Vec::new(),
        };
        let rows = Rows::new(&two, 1);
        assert!((rows.top(0, 10.0) - GROUP_GAP).abs() < f32::EPSILON);
        assert!((rows.top(1, 10.0) - (10.0 + GROUP_GAP)).abs() < f32::EPSILON);
        assert!((rows.top(2, 10.0) - (20.0 + 2.0 * GROUP_GAP)).abs() < f32::EPSILON);
        assert!((rows.height(10.0) - (40.0 + 2.0 * GROUP_GAP)).abs() < f32::EPSILON);
        let at = |y| rows.at(y, 10.0);
        assert_eq!(
            [
                at(5.0),
                at(15.0),
                at(30.0),
                at(40.0),
                at(50.0),
                at(67.0),
                at(70.0)
            ],
            [None, Some(0), Some(1), None, Some(2), Some(3), None]
        );
        let plain = Rows::plain(5, 2);
        assert_eq!(
            [
                plain.at(-1.0, 10.0),
                plain.at(25.0, 10.0),
                plain.at(31.0, 10.0)
            ],
            [None, Some(2), None]
        );
        assert!((plain.top(2, 10.0) - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn strips_show_the_color_of_each_row() {
        let tinted = |tints: Vec<Tint>| Arrangement {
            tints,
            ..Arrangement::plain(4)
        };
        let values = tinted(vec![
            Tint::Value(0),
            Tint::Value(1),
            Tint::Value(1),
            Tint::Other,
        ]);
        assert_eq!(
            Rows::new(&values, 1).strip,
            [
                Paint::Category(0),
                Paint::Category(1),
                Paint::Category(1),
                Paint::Line
            ]
        );
        assert_eq!(
            Rows::new(&values, 2).strip,
            [Paint::Category(0), Paint::Category(1)]
        );
        let levels = tinted(vec![
            Tint::Level(0),
            Tint::Level(100),
            Tint::Other,
            Tint::Other,
        ]);
        assert_eq!(Rows::new(&levels, 4).strip, [Paint::Level(50)]);
        assert_eq!(paint(Tint::Level(7)), Paint::Level(7));
        let rows = Rows::new(
            &Arrangement {
                groups: vec![group("A", 0, 4)],
                ..values
            },
            1,
        );
        let marks = rows.marks(10.0, 1.0);
        let strips: Vec<(f32, f32, f32, f32)> = marks
            .iter()
            .filter_map(|mark| match mark {
                Primitive::Rect {
                    x,
                    y,
                    width,
                    height,
                    ..
                } => Some((*x, *y, *width, *height)),
                _ => None,
            })
            .collect();
        assert_eq!(strips.len(), 4);
        assert_eq!(strips[1], (0.0, 10.0 + GROUP_GAP, STRIP, 9.0));
        let names: Vec<(String, f32)> = marks
            .iter()
            .filter_map(|mark| match mark {
                Primitive::Text { content, y, .. } => Some((content.to_string(), *y)),
                _ => None,
            })
            .collect();
        assert_eq!(names, [("A · 4 samples".to_owned(), GROUP_GAP / 2.0)]);
    }

    #[test]
    fn labels_count_the_samples_of_a_group() {
        assert_eq!(label("EUR", 1), "EUR · 1 sample");
        assert_eq!(label("AFR", 1_006), "AFR · 1,006 samples");
    }
}
