/// Compute fuzzy-match highlight indices for a query against a text.
///
/// Uses a simple greedy left-to-right case-insensitive match.
/// Returns the byte indices of each matched query character in the text.
/// This is only used for visual highlighting of the visible results.
pub fn find_match_indices(query: &str, text: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }

    let text_lower = text.to_lowercase();
    let mut indices = Vec::new();
    let mut t_pos = 0usize;

    for q_ch in query.to_lowercase().chars() {
        if let Some(found) = text_lower[t_pos..].find(q_ch) {
            let byte_pos = t_pos + found;
            // Convert the found position in the lowercase string to the
            // corresponding position in the original text. Since we're searching
            // for ASCII-alphabetic characters (query chars), and `find` returns
            // byte positions, the byte position is the same in both strings.
            indices.push(byte_pos);
            t_pos = byte_pos + q_ch.len_utf8();
        } else {
            return Vec::new();
        }
    }

    indices
}

/// Convert a list of byte indices into a list of (start, end) byte ranges,
/// merging contiguous ranges.
pub fn indices_to_ranges(indices: &[usize], text: &str) -> Vec<(usize, usize)> {
    if indices.is_empty() {
        return Vec::new();
    }

    let _char_indices: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .collect();

    let mut ranges = Vec::new();
    let mut start = indices[0];
    let mut end = start + char_at_byte_len(text, start);

    for &idx in &indices[1..] {
        let ch_len = char_at_byte_len(text, idx);
        if idx == end {
            end += ch_len;
        } else {
            ranges.push((start, end));
            start = idx;
            end = idx + ch_len;
        }
    }
    ranges.push((start, end));
    ranges
}

fn char_at_byte_len(s: &str, byte_pos: usize) -> usize {
    s[byte_pos..]
        .chars()
        .next()
        .map(|c| c.len_utf8())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_match_indices() {
        assert_eq!(find_match_indices("abc", "alphabetical"), vec![0, 5, 9]);
        assert_eq!(find_match_indices("lib", "library.rs"), vec![0, 1, 2]);
        assert_eq!(find_match_indices("sr", "src/main.rs"), vec![0, 1]);
        assert_eq!(find_match_indices("main", "src/main.rs"), vec![4, 5, 6, 7]);
    }

    #[test]
    fn test_indices_to_ranges() {
        assert_eq!(indices_to_ranges(&[0, 1, 2], "abc"), vec![(0, 3)]);
        assert_eq!(
            indices_to_ranges(&[0, 5, 7], "alphabetical"),
            vec![(0, 1), (5, 6), (7, 8)]
        );
    }
}
