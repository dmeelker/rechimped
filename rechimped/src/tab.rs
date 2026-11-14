/// The main screen's tabs, in the strip's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Week,
    Travel,
    Review,
    Report,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Week, Tab::Travel, Tab::Review, Tab::Report];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Week => "Time",
            Tab::Travel => "Travel",
            Tab::Report => "Report",
            Tab::Review => "Review",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).expect("tab in ALL")
    }

    /// The tab `step` places along the strip, wrapping around.
    pub fn cycled(self, step: isize) -> Tab {
        let len = Tab::ALL.len() as isize;
        Tab::ALL[(self.index() as isize + step).rem_euclid(len) as usize]
    }
}
