use crate::highlight::{find_match_indices, indices_to_ranges};
use crate::picker::FileResult;
use crate::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub struct UiState {
    pub query: String,
    pub highlight_query: String,
    pub results: Vec<FileResult>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub total_files: usize,
    pub total_matched: usize,
    pub is_scanning: bool,
    pub spinner_frame: usize,
}

pub fn draw(frame: &mut Frame, state: &UiState, theme: &Theme) {
    let area = frame.area();

    // Layout: status bar on top, results in middle, input at bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(3),    // results
            Constraint::Length(3), // input
        ])
        .split(area);

    draw_status_bar(frame, chunks[0], state, theme);
    draw_results(frame, chunks[1], state, theme);
    draw_input(frame, chunks[2], state, theme);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let spinner_chars = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let spinner = if state.is_scanning {
        spinner_chars[state.spinner_frame % spinner_chars.len()]
    } else {
        '✓'
    };

    let status_text = if state.is_scanning {
        format!(
            " {} Scanning… | {} files indexed | {} matches",
            spinner, state.total_files, state.total_matched
        )
    } else {
        format!(
            " {} Ready | {} files | {} matches",
            spinner, state.total_files, state.total_matched
        )
    };

    let paragraph = Paragraph::new(status_text).style(theme.style_status());
    frame.render_widget(paragraph, area);
}

fn draw_results(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::NONE)
        .style(theme.style_fg());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let visible_count = inner.height as usize;
    let max_scroll = state.results.len().saturating_sub(visible_count);
    let scroll = state.scroll_offset.min(max_scroll);
    let selected_idx = state.selected.min(state.results.len().saturating_sub(1));

    if visible_count == 0 || state.results.is_empty() {
        let placeholder = if state.is_scanning && state.results.is_empty() {
            "Scanning files…"
        } else if state.query.is_empty() {
            "Type to search files…"
        } else {
            "No matches found"
        };
        let para = Paragraph::new(placeholder)
            .style(theme.style_fg().add_modifier(Modifier::DIM))
            .alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(para, inner);
        return;
    }

    for (row, result_idx) in (scroll..(scroll + visible_count).min(state.results.len())).enumerate() {
        let result = &state.results[result_idx];
        let is_selected = result_idx == selected_idx;

        let row_area = Rect {
            x: inner.x,
            y: inner.y + row as u16,
            width: inner.width,
            height: 1,
        };

        let line = build_result_line(result, &state.highlight_query, theme, is_selected);
        let para = Paragraph::new(line);
        frame.render_widget(para, row_area);
    }
}

fn build_result_line(result: &FileResult, query: &str, theme: &Theme, is_selected: bool) -> Line<'static> {
    let path = &result.relative_path;
    let indices = find_match_indices(query, path);
    let ranges = indices_to_ranges(&indices, path);

    let base_style = if is_selected {
        theme.style_selected()
    } else {
        theme.style_fg()
    };

    let match_style = if is_selected {
        theme.style_match().bg(theme.selected_bg)
    } else {
        theme.style_match()
    };

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut last_end = 0usize;

    for (start, end) in ranges {
        if start > last_end {
            spans.push(Span::styled(
                path[last_end..start].to_string(),
                base_style,
            ));
        }
        spans.push(Span::styled(
            path[start..end].to_string(),
            match_style,
        ));
        last_end = end;
    }

    if last_end < path.len() {
        spans.push(Span::styled(path[last_end..].to_string(), base_style));
    }

    // If no ranges but we have text, show it all
    if spans.is_empty() && !path.is_empty() {
        spans.push(Span::styled(path.clone(), base_style));
    }

    // Add a small indicator for exact matches
    if result.exact_match {
        spans.push(Span::styled("  ✦", match_style));
    }

    Line::from(spans)
}

fn draw_input(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.style_prompt())
        .style(theme.style_fg());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let prompt = "> ";
    let prompt_width = prompt.width();
    let query = &state.query;

    // Simple cursor positioning: place cursor at end of query for now
    let cursor_x = inner.x + prompt_width as u16 + query.width() as u16;

    let spans = vec![
        Span::styled(prompt, theme.style_prompt()),
        Span::styled(query.clone(), theme.style_fg()),
    ];

    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, inner);

    // Draw cursor
    if cursor_x < inner.x + inner.width {
        frame.set_cursor_position((cursor_x, inner.y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_dump::buffer_to_string;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_state(is_scanning: bool, files: usize, matches: usize, results: Vec<FileResult>) -> UiState {
        UiState {
            query: String::new(),
            highlight_query: String::new(),
            results,
            selected: 0,
            scroll_offset: 0,
            total_files: files,
            total_matched: matches,
            is_scanning,
            spinner_frame: 0,
        }
    }

    #[test]
    fn test_scanning_placeholder() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = make_state(true, 0, 0, vec![]);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("Scanning…"),
            "expected 'Scanning…' in:\n{text}"
        );
        assert!(
            text.contains("Scanning files…"),
            "expected 'Scanning files…' placeholder in:\n{text}"
        );
    }

    #[test]
    fn test_ready_with_results() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![FileResult {
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            exact_match: false,
        }];
        let state = make_state(false, 42, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("Ready | 42 files | 1 matches"),
            "expected status bar in:\n{text}"
        );
        assert!(
            text.contains("src/main.rs"),
            "expected result row in:\n{text}"
        );
    }
}
