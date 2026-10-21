use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use crate::coverage::LineCoverage;
use crate::functions::LineSpan;

pub struct LcovReport {
    files: BTreeMap<String, BTreeMap<usize, bool>>,
}

impl LcovReport {
    const SOURCE_FILE: &str = "SF:";
    const LINE_HITS: &str = "DA:";

    pub fn parse(text: &str, root: &Path) -> Self {
        let mut report = Self {
            files: BTreeMap::new(),
        };
        let mut file = String::new();
        for record in text.lines() {
            if let Some(path) = record.strip_prefix(Self::SOURCE_FILE) {
                file = Self::relative(path, root);
            } else if let Some(line_hits) = record.strip_prefix(Self::LINE_HITS) {
                report.record(&file, line_hits);
            }
        }
        report
    }

    pub fn total(&self) -> LineCoverage {
        self.files
            .values()
            .flat_map(|lines| lines.values().copied())
            .collect()
    }

    pub fn within(&self, file: &str, span: LineSpan) -> LineCoverage {
        self.files
            .get(file)
            .map(|lines| {
                lines
                    .range(span.first..=span.last)
                    .map(|(_, covered)| *covered)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn most_missed(&self, count: usize) -> Vec<MissedLines> {
        let mut missed: Vec<MissedLines> = self
            .files
            .iter()
            .map(|(file, lines)| MissedLines::in_file(file, lines))
            .filter(|missed| !missed.lines.is_empty())
            .collect();
        missed.sort_by_key(|missed| std::cmp::Reverse(missed.lines.len()));
        missed.truncate(count);
        missed
    }

    fn record(&mut self, file: &str, line_hits: &str) {
        let mut fields = line_hits.split(',');
        let line = fields.next().and_then(|field| field.parse().ok());
        let hits = fields.next().and_then(|field| field.parse::<u64>().ok());
        if let (Some(line), Some(hits)) = (line, hits) {
            let covered = self
                .files
                .entry(file.to_string())
                .or_default()
                .entry(line)
                .or_default();
            *covered |= hits > 0;
        }
    }

    fn relative(path: &str, root: &Path) -> String {
        Path::new(path).strip_prefix(root).map_or_else(
            |_| path.to_string(),
            |relative| relative.display().to_string(),
        )
    }
}

pub struct MissedLines {
    file: String,
    lines: Vec<usize>,
}

impl MissedLines {
    fn in_file(file: &str, lines: &BTreeMap<usize, bool>) -> Self {
        Self {
            file: file.to_string(),
            lines: lines
                .iter()
                .filter(|(_, covered)| !**covered)
                .map(|(line, _)| *line)
                .collect(),
        }
    }

    fn ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for &line in &self.lines {
            match ranges.last_mut() {
                Some((_, last)) if *last + 1 == line => *last = line,
                _ => ranges.push((line, line)),
            }
        }
        ranges
    }
}

impl fmt::Display for MissedLines {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ranges: Vec<String> = self
            .ranges()
            .into_iter()
            .map(|(first, last)| {
                if first == last {
                    first.to_string()
                } else {
                    format!("{first}-{last}")
                }
            })
            .collect();
        write!(formatter, "{}: {}", self.file, ranges.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LcovReport;
    use crate::functions::LineSpan;

    struct SampleReport;

    impl SampleReport {
        const TEXT: &str = "SF:/work/project/src/shapes.rs\n\
                            DA:3,1\nDA:4,0\nDA:5,0\nDA:6,2\nDA:9,0\nend_of_record\n\
                            SF:/work/project/src/main.rs\nDA:1,4\nend_of_record\n";

        fn parse() -> LcovReport {
            LcovReport::parse(Self::TEXT, Path::new("/work/project"))
        }
    }

    #[test]
    fn total_counts_every_line_of_every_file() {
        assert_eq!(SampleReport::parse().total().percent(), 50.0);
    }

    #[test]
    fn within_counts_only_the_span() {
        let span = LineSpan { first: 5, last: 6 };
        assert_eq!(
            SampleReport::parse()
                .within("src/shapes.rs", span)
                .percent(),
            50.0
        );
    }

    #[test]
    fn an_unknown_file_has_no_coverage() {
        let span = LineSpan { first: 1, last: 9 };
        assert_eq!(
            SampleReport::parse().within("src/other.rs", span).percent(),
            0.0
        );
    }

    #[test]
    fn most_missed_lists_ranges_of_missed_lines() {
        let missed = SampleReport::parse().most_missed(5);
        let listed: Vec<String> = missed.iter().map(ToString::to_string).collect();
        assert_eq!(listed, ["src/shapes.rs: 4-5, 9"]);
    }
}
