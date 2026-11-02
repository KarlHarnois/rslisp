use super::measurement::Measurement;
use super::table::GroupedCount;

pub struct Change {
    before: u64,
    after: u64,
}

impl Change {
    pub const NOTABLE_RISE_PERCENT: f64 = 1.0;
    pub const PERCENT: f64 = 100.0;
    const WHOLE_PERCENT: u64 = 100;

    pub fn new(before: u64, after: u64) -> Self {
        Self { before, after }
    }

    pub fn of(before: &Measurement, after: &Measurement, stage: &str) -> Self {
        let count_of = |measurement: &Measurement| measurement.instructions_of(stage).unwrap_or(0);
        Self::new(count_of(before), count_of(after))
    }

    pub fn counts(&self) -> [String; 2] {
        [
            GroupedCount::new(self.before).to_string(),
            GroupedCount::new(self.after).to_string(),
        ]
    }

    pub fn exceeds_notable_rise(&self) -> bool {
        match self.ratio() {
            Some(percent) => percent > Self::NOTABLE_RISE_PERCENT,
            None => self.after > 0,
        }
    }

    pub fn differs(&self) -> bool {
        self.before != self.after
    }

    pub fn difference(&self) -> f64 {
        self.after as f64 - self.before as f64
    }

    pub fn percent(&self) -> String {
        match self.ratio() {
            Some(percent) => format!("{percent:+.2}%"),
            None if self.after == 0 => "0.00%".to_string(),
            None => "new".to_string(),
        }
    }

    pub fn rises_more_than_percent(&self, percent: u64) -> bool {
        let rise = u128::from(self.after.saturating_sub(self.before));
        rise * u128::from(Self::WHOLE_PERCENT) > u128::from(self.before) * u128::from(percent)
    }

    fn ratio(&self) -> Option<f64> {
        (self.before > 0).then(|| self.difference() / self.before as f64 * Self::PERCENT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_a_change_from_zero() {
        assert_eq!(Change::new(0, 0).percent(), "0.00%");
        assert_eq!(Change::new(0, 5).percent(), "new");
        assert_eq!(Change::new(200, 100).percent(), "-50.00%");
    }

    #[test]
    fn a_rise_of_exactly_the_percent_does_not_exceed_it() {
        assert!(!Change::new(1_000, 1_010).rises_more_than_percent(1));
        assert!(Change::new(1_000, 1_011).rises_more_than_percent(1));
    }

    #[test]
    fn a_fall_never_exceeds_the_percent() {
        assert!(!Change::new(1_000, 900).rises_more_than_percent(1));
    }
}
