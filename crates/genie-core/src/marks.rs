use crate::Region;

const PREFIX: &str = "Region ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub id: u64,
    pub name: String,
    pub contig: usize,
    pub span: Region,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    list: Vec<Mark>,
    last: u64,
}

impl Marks {
    pub fn add(&mut self, contig: usize, span: Region, name: Option<String>) -> u64 {
        if let Some(mark) = self
            .list
            .iter()
            .find(|mark| mark.contig == contig && mark.span == span)
        {
            return mark.id;
        }
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| self.fresh_name());
        self.last += 1;
        let key = |mark: &Mark| (mark.contig, mark.span.start(), mark.span.end());
        let at = self
            .list
            .partition_point(|mark| key(mark) <= (contig, span.start(), span.end()));
        self.list.insert(
            at,
            Mark {
                id: self.last,
                name,
                contig,
                span,
            },
        );
        self.last
    }

    pub fn remove(&mut self, id: u64) {
        self.list.retain(|mark| mark.id != id);
    }

    pub fn rename(&mut self, id: u64, name: &str) {
        let name = name.trim();
        if let Some(mark) = self.list.iter_mut().find(|mark| mark.id == id)
            && !name.is_empty()
        {
            name.clone_into(&mut mark.name);
        }
    }

    pub fn get(&self, id: u64) -> Option<&Mark> {
        self.list.iter().find(|mark| mark.id == id)
    }

    pub fn all(&self) -> &[Mark] {
        &self.list
    }

    pub fn on(&self, contig: usize) -> impl Iterator<Item = &Mark> {
        self.list.iter().filter(move |mark| mark.contig == contig)
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    fn fresh_name(&self) -> String {
        let highest = self
            .list
            .iter()
            .filter_map(|mark| mark.name.strip_prefix(PREFIX)?.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        format!("{PREFIX}{}", highest + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::Marks;
    use crate::Region;

    fn names(marks: &Marks) -> Vec<&str> {
        marks.all().iter().map(|mark| mark.name.as_str()).collect()
    }

    #[test]
    fn marks_stay_sorted_by_contig_and_start() {
        let mut marks = Marks::default();
        let late = marks.add(0, Region::between(500, 600), Some("late".to_owned()));
        let other = marks.add(1, Region::between(10, 20), Some("other".to_owned()));
        let early = marks.add(0, Region::between(100, 200), Some("early".to_owned()));
        assert_eq!(names(&marks), ["early", "late", "other"]);
        assert_ne!(early, late);
        assert_ne!(late, other);
        let on: Vec<&str> = marks.on(0).map(|mark| mark.name.as_str()).collect();
        assert_eq!(on, ["early", "late"]);
        assert_eq!(marks.get(other).unwrap().span, Region::between(10, 20));
    }

    #[test]
    fn unnamed_marks_count_up() {
        let mut marks = Marks::default();
        marks.add(0, Region::between(0, 10), None);
        let second = marks.add(0, Region::between(10, 20), Some("  ".to_owned()));
        marks.add(0, Region::between(20, 30), Some("Region 7".to_owned()));
        marks.add(0, Region::between(30, 40), None);
        assert_eq!(
            names(&marks),
            ["Region 1", "Region 2", "Region 7", "Region 8"]
        );
        marks.remove(second);
        assert_eq!(names(&marks), ["Region 1", "Region 7", "Region 8"]);
    }

    #[test]
    fn the_same_span_is_kept_once() {
        let mut marks = Marks::default();
        let first = marks.add(0, Region::between(0, 10), None);
        let again = marks.add(0, Region::between(0, 10), Some("again".to_owned()));
        assert_eq!(first, again);
        assert_eq!(names(&marks), ["Region 1"]);
    }

    #[test]
    fn renaming_ignores_empty_names() {
        let mut marks = Marks::default();
        let id = marks.add(0, Region::between(0, 10), None);
        marks.rename(id, " promoter A ");
        assert_eq!(names(&marks), ["promoter A"]);
        marks.rename(id, "   ");
        assert_eq!(names(&marks), ["promoter A"]);
        assert!(!marks.is_empty());
        marks.remove(id);
        assert!(marks.is_empty());
    }
}
