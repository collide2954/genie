//! Facts about the samples of a cohort, from a sample table: population, sex, phenotype and
//! so on. Tracks with a row for each sample use them to group, sort and color the rows.

use std::cmp::Ordering;
use std::collections::HashMap;

/// Values that mean that the table has no value, compared without case.
const MISSING: [&str; 6] = ["", ".", "na", "n/a", "nan", "null"];

/// A table with a row for each sample and a column for each field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    /// The names of the fields, without the sample column.
    pub columns: Vec<String>,
    rows: HashMap<String, usize>,
    /// The values of each row, one for each column. An empty value is a missing value.
    values: Vec<Vec<String>>,
    /// For each column: true when all its values are numbers.
    numbers: Vec<bool>,
}

impl Table {
    /// A table from the names of its fields and its rows. Each row has the name of a sample
    /// and a value for each field. Only the first row of a sample counts.
    pub fn new(columns: Vec<String>, rows: Vec<(String, Vec<String>)>) -> Self {
        let mut names = HashMap::new();
        let mut values: Vec<Vec<String>> = Vec::new();
        for (name, mut row) in rows {
            if name.is_empty() || names.contains_key(&name) {
                continue;
            }
            row.resize(columns.len(), String::new());
            for value in &mut row {
                let trimmed = value.trim();
                *value = if is_missing(trimmed) {
                    String::new()
                } else {
                    trimmed.to_owned()
                };
            }
            names.insert(name, values.len());
            values.push(row);
        }
        let numbers = (0..columns.len())
            .map(|column| {
                let mut found = values
                    .iter()
                    .map(|row| &row[column])
                    .filter(|value| !value.is_empty());
                let first = found.next();
                first.is_some_and(|first| number(first).is_some())
                    && found.all(|value| number(value).is_some())
            })
            .collect();
        Self {
            columns,
            rows: names,
            values,
            numbers,
        }
    }

    /// The number of samples.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn has(&self, sample: &str) -> bool {
        self.rows.contains_key(sample)
    }

    /// The number of a field, from its name.
    pub fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column == name)
    }

    /// The value of a field for a sample, or `None` when the table has no value.
    pub fn value(&self, sample: &str, column: usize) -> Option<&str> {
        let row = &self.values[*self.rows.get(sample)?];
        row.get(column)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    /// True when the values of a field are numbers.
    pub fn is_number(&self, column: usize) -> bool {
        self.numbers.get(column).copied().unwrap_or(false)
    }

    /// The fields of a sample that have a value, as names and values.
    pub fn fields(&self, sample: &str) -> Vec<(String, String)> {
        (0..self.columns.len())
            .filter_map(|column| {
                let value = self.value(sample, column)?;
                Some((self.columns[column].clone(), value.to_owned()))
            })
            .collect()
    }

    /// The number of the samples that the table knows.
    pub fn matches(&self, samples: &[String]) -> usize {
        samples.iter().filter(|sample| self.has(sample)).count()
    }
}

fn is_missing(value: &str) -> bool {
    MISSING
        .iter()
        .any(|missing| value.eq_ignore_ascii_case(missing))
}

/// A number from a value, if the value is a finite number.
pub fn number(value: &str) -> Option<f64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// The most values of a field that get a color of their own.
pub const COLORS: usize = 8;
/// The most values of a field that get a group of their own.
pub const GROUPS: usize = 30;

/// The fields that group, sort and color the rows of the samples of a track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    pub group: Option<String>,
    pub sort: Option<String>,
    pub color: Option<String>,
}

impl Choice {
    pub fn is_empty(&self) -> bool {
        self.group.is_none() && self.sort.is_none() && self.color.is_none()
    }
}

/// The samples of a group: its name, the place of its first sample in the order of the rows,
/// and its number of samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub first: usize,
    pub size: usize,
}

/// The color of the row of a sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    /// One of the most common values of the field: its place among them, from 0.
    Value(u8),
    /// A number: from 0 for the smallest to 100 for the largest.
    Level(u8),
    /// No value, or a value that is not one of the most common values.
    Other,
}

/// The order, the groups and the colors of the samples of a track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Arrangement {
    /// The samples in the order of the rows, by their number in the file of the track.
    pub order: Vec<usize>,
    pub groups: Vec<Group>,
    /// The color of each sample by its number in the file, or empty when no field colors the
    /// rows.
    pub tints: Vec<Tint>,
}

impl Arrangement {
    /// The samples in the order of the file, without groups or colors.
    pub fn plain(samples: usize) -> Self {
        Self {
            order: (0..samples).collect(),
            groups: Vec::new(),
            tints: Vec::new(),
        }
    }
}

/// Where a sample goes by the value of a field: values first, then samples without a value,
/// then samples that are not in the table.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum Key<'a> {
    Number(f64),
    Text(String, &'a str),
    Missing,
    Absent,
}

fn key(value: &str, numbers: bool) -> Key<'_> {
    if numbers {
        number(value).map_or(Key::Missing, Key::Number)
    } else {
        Key::Text(value.to_lowercase(), value)
    }
}

fn sample_key<'a>(table: &'a Table, sample: &str, column: usize, numbers: bool) -> Key<'a> {
    if !table.has(sample) {
        return Key::Absent;
    }
    table
        .value(sample, column)
        .map_or(Key::Missing, |value| key(value, numbers))
}

fn compare(a: &Key<'_>, b: &Key<'_>) -> Ordering {
    a.partial_cmp(b).unwrap_or(Ordering::Equal)
}

/// The values of a field among some samples, each with its number of samples.
fn counts<'a>(table: &'a Table, samples: &[String], column: usize) -> Vec<(&'a str, usize)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for value in samples
        .iter()
        .filter_map(|sample| table.value(sample, column))
    {
        *counts.entry(value).or_default() += 1;
    }
    let numbers = table.is_number(column);
    let mut counts: Vec<(&str, usize)> = counts.into_iter().collect();
    counts.sort_by(|(a, first), (b, second)| {
        second
            .cmp(first)
            .then_with(|| compare(&key(a, numbers), &key(b, numbers)))
    });
    counts
}

/// The order, the groups and the colors of the samples of a track by the fields of a table.
pub fn arrange(samples: &[String], table: &Table, choice: &Choice) -> Arrangement {
    let column = |name: &Option<String>| name.as_deref().and_then(|name| table.column(name));
    let mut arrangement = Arrangement::plain(samples.len());
    if let Some(sort) = column(&choice.sort) {
        let numbers = table.is_number(sort);
        let keys: Vec<Key<'_>> = samples
            .iter()
            .map(|sample| sample_key(table, sample, sort, numbers))
            .collect();
        arrangement
            .order
            .sort_by(|&a, &b| compare(&keys[a], &keys[b]));
    }
    if let Some(group) = column(&choice.group) {
        let (order, groups) = grouped(samples, table, group, &arrangement.order);
        arrangement.order = order;
        arrangement.groups = groups;
    }
    if let Some(color) = column(&choice.color) {
        arrangement.tints = tints(samples, table, color);
    }
    arrangement
}

/// The samples in groups by the values of a field, each group in the order of `order`. The
/// groups of the most common values come in the order of their values, then the group of the
/// other values, the group without a value and the group of the samples that the table does
/// not know.
fn grouped(
    samples: &[String],
    table: &Table,
    column: usize,
    order: &[usize],
) -> (Vec<usize>, Vec<Group>) {
    let numbers = table.is_number(column);
    let mut kept: Vec<&str> = counts(table, samples, column)
        .into_iter()
        .take(GROUPS)
        .map(|(value, _)| value)
        .collect();
    kept.sort_by(|a, b| compare(&key(a, numbers), &key(b, numbers)));
    let places: HashMap<&str, usize> = kept
        .iter()
        .enumerate()
        .map(|(place, value)| (*value, place))
        .collect();
    let field = &table.columns[column];
    let mut names: Vec<String> = kept.iter().map(|value| (*value).to_owned()).collect();
    let (other, missing, absent) = (names.len(), names.len() + 1, names.len() + 2);
    names.push(format!("Other {field} values"));
    names.push(format!("No {field}"));
    names.push("Not in the sample table".to_owned());
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    for &sample in order {
        let name = &samples[sample];
        let place = match table.value(name, column) {
            Some(value) => places.get(value).copied().unwrap_or(other),
            None if table.has(name) => missing,
            None => absent,
        };
        buckets[place].push(sample);
    }
    let mut ordered = Vec::with_capacity(order.len());
    let mut groups = Vec::new();
    for (bucket, name) in buckets.into_iter().zip(names) {
        if bucket.is_empty() {
            continue;
        }
        groups.push(Group {
            name,
            first: ordered.len(),
            size: bucket.len(),
        });
        ordered.extend(bucket);
    }
    (ordered, groups)
}

/// The color of each sample by a field: the most common values get colors, and numbers get a
/// level between the smallest and the largest number.
fn tints(samples: &[String], table: &Table, column: usize) -> Vec<Tint> {
    if table.is_number(column) {
        let values: Vec<Option<f64>> = samples
            .iter()
            .map(|sample| table.value(sample, column).and_then(number))
            .collect();
        let (low, high) = values
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
                (low.min(*value), high.max(*value))
            });
        return values
            .iter()
            .map(|value| match value {
                Some(value) if high > low => {
                    let share = (value - low) / (high - low);
                    Tint::Level((share * 100.0).round().clamp(0.0, 100.0) as u8)
                }
                Some(_) => Tint::Level(100),
                None => Tint::Other,
            })
            .collect();
    }
    let colored: HashMap<&str, u8> = counts(table, samples, column)
        .into_iter()
        .take(COLORS)
        .zip(0..)
        .map(|((value, _), place)| (value, place))
        .collect();
    samples
        .iter()
        .map(|sample| {
            table
                .value(sample, column)
                .and_then(|value| colored.get(value).copied())
                .map_or(Tint::Other, Tint::Value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Arrangement, Choice, GROUPS, Table, Tint, arrange};

    fn row(name: &str, values: &[&str]) -> (String, Vec<String>) {
        (
            name.to_owned(),
            values.iter().map(|value| (*value).to_owned()).collect(),
        )
    }

    fn table() -> Table {
        let columns = ["pop", "sex", "age"].map(str::to_owned).to_vec();
        Table::new(
            columns,
            vec![
                row("NA1", &["EUR", "female", "31"]),
                row("NA2", &[" AFR ", "NA", "45.5"]),
                row("NA1", &["EAS", "male", "60"]),
                row("NA3", &["EAS"]),
                row("", &["SAS", "male", "20"]),
                row("NA4", &["n/a", ".", ""]),
            ],
        )
    }

    #[test]
    fn tables_find_the_values_of_a_sample() {
        let table = table();
        assert_eq!(table.len(), 4);
        assert_eq!(table.column("sex"), Some(1));
        assert_eq!(table.column("Sex"), None);
        assert_eq!(table.value("NA1", 0), Some("EUR"));
        assert_eq!(table.value("NA2", 0), Some("AFR"));
        assert_eq!(table.value("NA2", 1), None);
        assert_eq!(table.value("NA3", 2), None);
        assert_eq!(table.value("NA4", 0), None);
        assert_eq!(table.value("NA9", 0), None);
        assert_eq!(table.value("NA1", 7), None);
        assert_eq!(
            table.fields("NA2"),
            [
                ("pop".to_owned(), "AFR".to_owned()),
                ("age".to_owned(), "45.5".to_owned())
            ]
        );
        assert_eq!(table.fields("NA4"), Vec::<(String, String)>::new());
        assert!(table.has("NA3") && !table.has("NA9") && !table.has(""));
        let names = ["NA1", "NA9", "NA3"].map(str::to_owned);
        assert_eq!(table.matches(&names), 2);
    }

    #[test]
    fn number_fields_hold_only_numbers() {
        let table = table();
        assert!(!table.is_number(0));
        assert!(!table.is_number(1));
        assert!(table.is_number(2));
        assert!(!table.is_number(3));
        let empty = Table::new(vec!["x".to_owned()], vec![row("S1", &["NA"])]);
        assert!(!empty.is_number(0));
    }

    /// Six samples of a table, and a seventh sample that the table does not know.
    fn cohort() -> (Vec<String>, Table) {
        let columns = ["pop", "age", "sex"].map(str::to_owned).to_vec();
        let table = Table::new(
            columns,
            vec![
                row("S1", &["EUR", "31", "female"]),
                row("S2", &["AFR", "45.5", "male"]),
                row("S3", &["EUR", "9", "female"]),
                row("S4", &["", "20", "male"]),
                row("S5", &["AFR", "", "female"]),
                row("S6", &["EAS", "100", ""]),
            ],
        );
        let samples = ["S1", "S2", "S3", "S4", "S5", "S6", "X"]
            .map(str::to_owned)
            .to_vec();
        (samples, table)
    }

    fn by(group: Option<&str>, sort: Option<&str>, color: Option<&str>) -> Choice {
        Choice {
            group: group.map(str::to_owned),
            sort: sort.map(str::to_owned),
            color: color.map(str::to_owned),
        }
    }

    fn groups(arrangement: &Arrangement) -> Vec<(&str, usize, usize)> {
        arrangement
            .groups
            .iter()
            .map(|group| (group.name.as_str(), group.first, group.size))
            .collect()
    }

    #[test]
    fn groups_follow_the_values_of_a_field() {
        let (samples, table) = cohort();
        let grouped = arrange(&samples, &table, &by(Some("pop"), None, None));
        assert_eq!(grouped.order, [1, 4, 5, 0, 2, 3, 6]);
        assert_eq!(
            groups(&grouped),
            [
                ("AFR", 0, 2),
                ("EAS", 2, 1),
                ("EUR", 3, 2),
                ("No pop", 5, 1),
                ("Not in the sample table", 6, 1)
            ]
        );
        assert_eq!(grouped.tints, []);
        assert!(!by(Some("pop"), None, None).is_empty() && Choice::default().is_empty());
        let plain = arrange(&samples, &table, &Choice::default());
        assert_eq!(plain, Arrangement::plain(7));
        let unknown = arrange(&samples, &table, &by(Some("height"), Some("weight"), None));
        assert_eq!(unknown, Arrangement::plain(7));
    }

    #[test]
    fn sorting_orders_the_rows_inside_each_group() {
        let (samples, table) = cohort();
        let ages = arrange(&samples, &table, &by(None, Some("age"), None));
        assert_eq!(ages.order, [2, 3, 0, 1, 5, 4, 6]);
        assert_eq!(ages.groups, []);
        let both = arrange(&samples, &table, &by(Some("pop"), Some("age"), None));
        assert_eq!(both.order, [1, 4, 5, 2, 0, 3, 6]);
        let words = arrange(&samples, &table, &by(None, Some("sex"), None));
        assert_eq!(words.order, [0, 2, 4, 1, 3, 5, 6]);
    }

    #[test]
    fn rare_values_share_one_group() {
        let rows: Vec<(String, Vec<String>)> = (0..40)
            .map(|index| {
                let value = if index < 4 {
                    "big".to_owned()
                } else {
                    format!("v{index:02}")
                };
                (format!("S{index}"), vec![value])
            })
            .collect();
        let samples: Vec<String> = rows.iter().map(|(name, _)| name.clone()).collect();
        let table = Table::new(vec!["id".to_owned()], rows);
        let grouped = arrange(&samples, &table, &by(Some("id"), None, None));
        assert_eq!(grouped.groups.len(), GROUPS + 1);
        assert_eq!(groups(&grouped)[0], ("big", 0, 4));
        assert_eq!(groups(&grouped)[1], ("v04", 4, 1));
        assert_eq!(groups(&grouped)[GROUPS], ("Other id values", 33, 7));
        assert_eq!(grouped.order.len(), 40);
    }

    #[test]
    fn colors_go_to_the_most_common_values_and_levels_to_numbers() {
        let kinds = [
            "a", "a", "a", "b", "b", "c", "d", "e", "f", "g", "h", "i", "",
        ];
        let rows = kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| row(&format!("S{index}"), &[*kind]))
            .collect();
        let table = Table::new(vec!["kind".to_owned()], rows);
        let samples: Vec<String> = (0..kinds.len()).map(|index| format!("S{index}")).collect();
        let tints = arrange(&samples, &table, &by(None, None, Some("kind"))).tints;
        assert_eq!(
            tints,
            [
                Tint::Value(0),
                Tint::Value(0),
                Tint::Value(0),
                Tint::Value(1),
                Tint::Value(1),
                Tint::Value(2),
                Tint::Value(3),
                Tint::Value(4),
                Tint::Value(5),
                Tint::Value(6),
                Tint::Value(7),
                Tint::Other,
                Tint::Other
            ]
        );
        let (samples, table) = cohort();
        let colored = arrange(&samples, &table, &by(None, None, Some("age")));
        assert_eq!(colored.order, [0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            colored.tints,
            [
                Tint::Level(24),
                Tint::Level(40),
                Tint::Level(0),
                Tint::Level(12),
                Tint::Other,
                Tint::Level(100),
                Tint::Other
            ]
        );
    }
}
