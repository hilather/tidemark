//! Owned by: W7-A
//! Pure browser navigation and page-local ordering.

use tidemark_core::snapshot::DirRow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pane {
    Main,
    Info,
    Files,
    Links,
    Instances,
    Picker,
    Help,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sort {
    Size,
    Reclaim,
    Files,
    Newest,
    Growth,
    Name,
}
impl Sort {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Size => Self::Reclaim,
            Self::Reclaim => Self::Files,
            Self::Files => Self::Newest,
            Self::Newest => Self::Growth,
            Self::Growth => Self::Name,
            Self::Name => Self::Size,
        }
    }
}
#[derive(Debug)]
pub(crate) struct State {
    pub(crate) selected: usize,
    pub(crate) pane: Pane,
    pub(crate) sort: Sort,
    pub(crate) filter: String,
    pub(crate) editing: bool,
    pub(crate) page_start: u32,
    pub(crate) next: Option<u32>,
}
impl State {
    pub(crate) fn new(start: u32) -> Self {
        Self {
            selected: 0,
            pane: Pane::Main,
            sort: Sort::Size,
            filter: String::new(),
            editing: false,
            page_start: start,
            next: None,
        }
    }
    pub(crate) fn move_by(&mut self, amount: isize, count: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(count.saturating_sub(1));
    }
    pub(crate) fn reset(&mut self, start: u32) {
        self.selected = 0;
        self.page_start = start;
        self.next = None;
        self.pane = Pane::Main;
    }
    pub(crate) fn order(&self, rows: &mut [(DirRow, Option<i128>)]) {
        rows.sort_by(|a, b| {
            let order = match self.sort {
                Sort::Size => b.0.t_disk.cmp(&a.0.t_disk),
                Sort::Reclaim => b.0.t_reclaimable.cmp(&a.0.t_reclaimable),
                Sort::Files => b.0.t_files.cmp(&a.0.t_files),
                Sort::Newest => b.0.newest_mtime.cmp(&a.0.newest_mtime),
                Sort::Growth => b.1.cmp(&a.1),
                Sort::Name => a.0.name.cmp(&b.0.name),
            };
            order.then_with(|| a.0.name.cmp(&b.0.name))
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movement_is_bounded() {
        let mut s = State::new(1);
        s.move_by(-1, 0);
        assert_eq!(s.selected, 0);
        s.move_by(999, 3);
        assert_eq!(s.selected, 2);
        s.move_by(-999, 3);
        assert_eq!(s.selected, 0);
        s.reset(20);
        assert_eq!(s.page_start, 20);
    }
    #[test]
    fn sort_cycles() {
        let mut s = Sort::Size;
        for _ in 0..6 {
            s = s.next();
        }
        assert_eq!(s, Sort::Size);
    }
}
