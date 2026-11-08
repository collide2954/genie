use crate::Region;

const LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub contig: usize,
    pub region: Region,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    back: Vec<Visit>,
    forward: Vec<Visit>,
}

impl History {
    pub fn record(&mut self, visit: Visit) {
        self.forward.clear();
        if self.back.last() == Some(&visit) {
            return;
        }
        self.back.push(visit);
        if self.back.len() > LIMIT {
            self.back.remove(0);
        }
    }

    pub fn back(&mut self, current: Visit) -> Option<Visit> {
        let visit = self.back.pop()?;
        self.forward.push(current);
        Some(visit)
    }

    pub fn forward(&mut self, current: Visit) -> Option<Visit> {
        let visit = self.forward.pop()?;
        self.back.push(current);
        Some(visit)
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Visit};
    use crate::Region;

    fn visit(contig: usize, start: u64) -> Visit {
        Visit {
            contig,
            region: Region::between(start, start + 100),
        }
    }

    #[test]
    fn back_and_forward_return_the_visits() {
        let mut history = History::default();
        assert!(!history.can_go_back() && !history.can_go_forward());
        history.record(visit(0, 0));
        history.record(visit(0, 500));
        assert_eq!(history.back(visit(1, 0)), Some(visit(0, 500)));
        assert_eq!(history.back(visit(0, 500)), Some(visit(0, 0)));
        assert_eq!(history.back(visit(0, 0)), None);
        assert_eq!(history.forward(visit(0, 0)), Some(visit(0, 500)));
        assert_eq!(history.forward(visit(0, 500)), Some(visit(1, 0)));
        assert_eq!(history.forward(visit(1, 0)), None);
        assert!(history.can_go_back() && !history.can_go_forward());
    }

    #[test]
    fn a_new_visit_drops_the_forward_steps() {
        let mut history = History::default();
        history.record(visit(0, 0));
        assert_eq!(history.back(visit(0, 500)), Some(visit(0, 0)));
        assert!(history.can_go_forward());
        history.record(visit(0, 0));
        assert!(!history.can_go_forward());
    }

    #[test]
    fn repeated_visits_are_kept_once() {
        let mut history = History::default();
        history.record(visit(0, 0));
        history.record(visit(0, 0));
        assert_eq!(history.back(visit(0, 500)), Some(visit(0, 0)));
        assert!(!history.can_go_back());
    }

    #[test]
    fn history_keeps_the_last_hundred_visits() {
        let mut history = History::default();
        for start in 0..150 {
            history.record(visit(0, start));
        }
        let mut count = 0;
        let mut current = visit(1, 0);
        while let Some(previous) = history.back(current) {
            current = previous;
            count += 1;
        }
        assert_eq!(count, 100);
        assert_eq!(current, visit(0, 50));
    }
}
