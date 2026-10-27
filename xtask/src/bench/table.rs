use std::fmt;

pub struct MarkdownTable {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl MarkdownTable {
    const TEXT_COLUMNS: usize = 2;

    pub fn new(header: &[&str]) -> Self {
        Self {
            header: header.iter().map(|title| title.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    fn write_row(formatter: &mut fmt::Formatter<'_>, cells: &[String]) -> fmt::Result {
        writeln!(formatter, "| {} |", cells.join(" | "))
    }

    fn alignment_row(&self) -> Vec<String> {
        let mut alignments = vec!["---".to_string(); self.header.len()];
        alignments
            .iter_mut()
            .skip(Self::TEXT_COLUMNS)
            .for_each(|cell| *cell = "---:".to_string());
        alignments
    }
}

impl fmt::Display for MarkdownTable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        Self::write_row(formatter, &self.header)?;
        Self::write_row(formatter, &self.alignment_row())?;
        self.rows
            .iter()
            .try_for_each(|row| Self::write_row(formatter, row))
    }
}

pub struct GroupedCount(pub u64);

impl GroupedCount {
    const GROUP: usize = 3;
}

impl fmt::Display for GroupedCount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = self.0.to_string();
        let first_group = match digits.len() % Self::GROUP {
            0 => Self::GROUP,
            remainder => remainder,
        };
        formatter.write_str(&digits[..first_group])?;
        digits.as_bytes()[first_group..]
            .chunks(Self::GROUP)
            .try_for_each(|group| write!(formatter, ",{}", String::from_utf8_lossy(group)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_digits_in_thousands() {
        assert_eq!(GroupedCount(0).to_string(), "0");
        assert_eq!(GroupedCount(999).to_string(), "999");
        assert_eq!(GroupedCount(1_234_567).to_string(), "1,234,567");
    }

    #[test]
    fn right_aligns_the_count_columns() {
        let mut table = MarkdownTable::new(&["Program", "Stage", "Instructions"]);
        table.push(vec!["inventory".into(), "emit".into(), "12".into()]);
        assert_eq!(
            table.to_string(),
            "| Program | Stage | Instructions |\n| --- | --- | ---: |\n| inventory | emit | 12 |\n"
        );
    }
}
