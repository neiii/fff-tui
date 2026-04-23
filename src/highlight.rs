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

// ─── Syntax highlighting via syntect ────────────────────────────────────────

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Highlight file content with syntect. Returns plain text if highlighting fails.
pub fn highlight_content(path: &str, content: &str) -> Text<'static> {
    let ss = syntax_set();
    let ts = theme_set();

    let syntax = ss
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let theme = ts.themes.get("base16-ocean.dark").unwrap_or_else(|| {
        ts.themes.values().next().expect("no default themes")
    });

    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut lines = Vec::new();
    for line in LinesWithEndings::from(content) {
        match highlighter.highlight_line(line, ss) {
            Ok(regions) => {
                let spans: Vec<Span> = regions
                    .into_iter()
                    .map(|(style, text)| span_from_syntect(style, text))
                    .collect();
                lines.push(Line::from(spans));
            }
            Err(_) => {
                lines.push(Line::from(line.trim_end_matches('\n').to_string()));
            }
        }
    }

    Text::from(lines)
}

fn span_from_syntect(style: SyntectStyle, text: &str) -> Span<'static> {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut ratatui_style = Style::default().fg(fg);
    if style.font_style.contains(syntect::highlighting::FontStyle::BOLD) {
        ratatui_style = ratatui_style.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::ITALIC) {
        ratatui_style = ratatui_style.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(syntect::highlighting::FontStyle::UNDERLINE) {
        ratatui_style = ratatui_style.add_modifier(Modifier::UNDERLINED);
    }
    Span::styled(text.to_string(), ratatui_style)
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

    #[test]
    fn test_highlight_content_basic() {
        let text = highlight_content("test.rs", "fn main() {}");
        assert!(!text.lines.is_empty());
    }
}
