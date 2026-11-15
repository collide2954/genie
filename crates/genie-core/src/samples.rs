//! Facts about the samples of a cohort, from a sample table: population, sex, phenotype and
//! so on. Tracks with a row for each sample use them to group, sort and color the rows.

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

#[cfg(test)]
mod tests {
    use super::Table;

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
}
