use std::rc::Rc;

pub struct SimilarName;

impl SimilarName {
    const LENGTH_PER_ALLOWED_EDIT: usize = 3;

    pub fn closest(target: &str, candidates: &[Rc<str>]) -> Option<Rc<str>> {
        let length = target.chars().count();
        let allowed_edits = (length / Self::LENGTH_PER_ALLOWED_EDIT).max(1);
        let shares_a_character = |distance: usize| distance < length;
        candidates
            .iter()
            .filter(|candidate| candidate.as_ref() != target)
            .filter(|candidate| length.abs_diff(candidate.chars().count()) <= allowed_edits)
            .map(|candidate| (Self::edit_distance(target, candidate), candidate))
            .filter(|(distance, _)| *distance <= allowed_edits && shares_a_character(*distance))
            .min_by(|(left_distance, left), (right_distance, right)| {
                left_distance.cmp(right_distance).then(left.cmp(right))
            })
            .map(|(_, candidate)| Rc::clone(candidate))
    }

    fn edit_distance(left: &str, right: &str) -> usize {
        let left: Vec<char> = left.chars().collect();
        let right: Vec<char> = right.chars().collect();
        let mut distances = vec![vec![0; right.len() + 1]; left.len() + 1];
        (0..=left.len()).for_each(|row| distances[row][0] = row);
        (0..=right.len()).for_each(|column| distances[0][column] = column);
        for row in 1..=left.len() {
            for column in 1..=right.len() {
                distances[row][column] =
                    Self::cheapest_edit(&distances, &left, &right, row, column);
            }
        }
        distances[left.len()][right.len()]
    }

    fn cheapest_edit(
        distances: &[Vec<usize>],
        left: &[char],
        right: &[char],
        row: usize,
        column: usize,
    ) -> usize {
        let substitution =
            distances[row - 1][column - 1] + usize::from(left[row - 1] != right[column - 1]);
        let deletion = distances[row - 1][column] + 1;
        let insertion = distances[row][column - 1] + 1;
        let best = substitution.min(deletion).min(insertion);
        let is_transposition = row > 1
            && column > 1
            && left[row - 1] == right[column - 2]
            && left[row - 2] == right[column - 1];
        if is_transposition {
            best.min(distances[row - 2][column - 2] + 1)
        } else {
            best
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<Rc<str>> {
        list.iter().map(|name| Rc::from(*name)).collect()
    }

    #[test]
    fn suggests_a_name_one_edit_away() {
        let candidates = names(&["point-x", "point-y", "println"]);
        assert_eq!(
            SimilarName::closest("pointx", &candidates).as_deref(),
            Some("point-x")
        );
    }

    #[test]
    fn counts_swapped_letters_as_one_edit() {
        let candidates = names(&["width", "height"]);
        assert_eq!(
            SimilarName::closest("widht", &candidates).as_deref(),
            Some("width")
        );
    }

    #[test]
    fn suggests_nothing_for_a_name_sharing_no_character() {
        let candidates = names(&["x", "y"]);
        assert_eq!(SimilarName::closest("*", &candidates), None);
    }

    #[test]
    fn suggests_nothing_when_every_name_is_far() {
        let candidates = names(&["defstruct", "println"]);
        assert_eq!(SimilarName::closest("area", &candidates), None);
    }
}
