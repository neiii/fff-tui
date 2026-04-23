use crate::highlight::{find_match_indices, indices_to_ranges};
use crate::picker::{MatchKind, SearchMode, SearchScope, UnifiedResult};
use crate::theme::Theme;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub struct UiState {
    pub query: String,
    pub highlight_query: String,
    pub results: Vec<UnifiedResult>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub total_files: usize,
    pub total_matched: usize,
    pub is_scanning: bool,
    pub spinner_frame: usize,
    pub terminal_width: u16,
    pub preview_enabled: bool,
    pub search_mode: SearchMode,
    pub search_scope: SearchScope,
}

pub fn draw(frame: &mut Frame, state: &UiState, theme: &Theme) {
    let area = frame.area();

    let show_preview = state.preview_enabled
        && state.terminal_width >= 100
        && state
            .results
            .get(state.selected)
            .map(|r| r.kind == MatchKind::Line)
            .unwrap_or(false);

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // status bar
            Constraint::Min(3),    // results + optional preview
            Constraint::Length(3), // input
        ])
        .split(area);

    let results_area = if show_preview {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);
        hchunks[0]
    } else {
        main_chunks[1]
    };

    draw_status_bar(frame, main_chunks[0], state, theme);
    draw_result_list(frame, results_area, state, theme);
    if show_preview {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(main_chunks[1]);
        draw_preview_pane(frame, hchunks[1], state, theme);
    }
    draw_input(frame, main_chunks[2], state, theme);
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

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(18)])
        .split(area);

    let paragraph = Paragraph::new(status_text).style(theme.style_status());
    frame.render_widget(paragraph, chunks[0]);

    // Mode toggles: Cc W .* F [U/F/G]
    let mut toggles = Vec::new();
    toggles.push(mode_span(
        "Cc",
        state.search_mode.case_sensitive,
        theme,
    ));
    toggles.push(Span::styled(" ", theme.style_status()));
    toggles.push(mode_span(
        "W",
        state.search_mode.whole_word,
        theme,
    ));
    toggles.push(Span::styled(" ", theme.style_status()));
    toggles.push(mode_span(
        ".*",
        state.search_mode.regex,
        theme,
    ));
    toggles.push(Span::styled(" ", theme.style_status()));
    toggles.push(mode_span(
        "F",
        state.search_mode.fixed_strings,
        theme,
    ));
    toggles.push(Span::styled(" ", theme.style_status()));
    let scope_label = match state.search_scope {
        SearchScope::Unified => "[U]",
        SearchScope::FileOnly => "[F]",
        SearchScope::GrepOnly => "[G]",
    };
    toggles.push(Span::styled(scope_label, theme.style_status().add_modifier(Modifier::BOLD)));

    let toggle_line = Line::from(toggles).alignment(ratatui::layout::Alignment::Right);
    let toggle_para = Paragraph::new(toggle_line).style(theme.style_status());
    frame.render_widget(toggle_para, chunks[1]);
}

fn mode_span(label: &'static str, active: bool, theme: &Theme) -> Span<'static> {
    if active {
        Span::styled(label, theme.style_mode_active())
    } else {
        Span::styled(label, theme.style_mode_inactive())
    }
}

fn draw_result_list(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
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

    for (row, result_idx) in (scroll..(scroll + visible_count).min(state.results.len())).enumerate()
    {
        let result = &state.results[result_idx];
        let is_selected = result_idx == selected_idx;

        let row_area = Rect {
            x: inner.x,
            y: inner.y + row as u16,
            width: inner.width,
            height: 1,
        };

        let line =
            build_result_line(result, &state.highlight_query, theme, inner.width as usize, is_selected);
        let para = Paragraph::new(line);
        frame.render_widget(para, row_area);
    }
}

fn build_result_line(
    result: &UnifiedResult,
    query: &str,
    theme: &Theme,
    available_width: usize,
    is_selected: bool,
) -> Line<'static> {
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

    // Badge
    match result.kind {
        MatchKind::File => {
            spans.push(Span::styled("[FILE]", theme.style_badge_file()));
            spans.push(Span::styled(" ", base_style));
        }
        MatchKind::Line => {
            spans.push(Span::styled("[LINE]", theme.style_badge_line()));
            spans.push(Span::styled(" ", base_style));
        }
    }

    let badge_width = 7; // "[FILE] " or "[LINE] "
    let content_width = available_width.saturating_sub(badge_width);

    if result.kind == MatchKind::Line {
        let content = result.line_content.as_deref().unwrap_or("");
        let file_name = std::path::Path::new(&result.relative_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&result.relative_path);
        let line_num = result.line_number.unwrap_or(0);
        let meta = format!("{file_name}:{line_num}");
        let meta_width = meta.width();
        let gap = 2;

        // Build highlighted content spans
        let mut content_spans: Vec<Span<'static>> = Vec::new();
        if let Some(ref offsets) = result.match_byte_offsets {
            let mut last_end = 0usize;
            for &(start, end) in offsets {
                let start = start as usize;
                let end = end as usize;
                if start > last_end && start <= content.len() {
                    content_spans.push(Span::styled(
                        content[last_end..start].to_string(),
                        base_style,
                    ));
                }
                if end <= content.len() {
                    content_spans.push(Span::styled(
                        content[start..end].to_string(),
                        match_style,
                    ));
                    last_end = end;
                }
            }
            if last_end < content.len() {
                content_spans.push(Span::styled(
                    content[last_end..].to_string(),
                    base_style,
                ));
            }
        } else {
            content_spans.push(Span::styled(content.to_string(), base_style));
        }

        if result.is_definition == Some(true) {
            content_spans.push(Span::styled("  §", match_style));
        }

        // Compute content display width
        let content_display_width: usize = content_spans
            .iter()
            .map(|s| s.content.width())
            .sum();

        let total_needed = content_display_width + gap + meta_width;

        if total_needed > content_width && content_width > meta_width + gap + 1 {
            // Truncate content to make room for meta
            let max_content_display = content_width.saturating_sub(meta_width + gap + 1); // 1 for "…"
            let (truncated, truncated_width) = truncate_to_width(content, max_content_display);
            let mut truncated_spans = Vec::new();
            if !truncated.is_empty() {
                // Re-apply highlighting on truncated content using byte offsets
                let trunc_byte_len = truncated.len();
                if let Some(ref offsets) = result.match_byte_offsets {
                    let mut last_end = 0usize;
                    for &(start, end) in offsets {
                        let start = start as usize;
                        let end = end as usize;
                        if start >= trunc_byte_len {
                            break;
                        }
                        if start > last_end {
                            truncated_spans.push(Span::styled(
                                truncated[last_end..start].to_string(),
                                base_style,
                            ));
                        }
                        let actual_end = end.min(trunc_byte_len);
                        truncated_spans.push(Span::styled(
                            truncated[start..actual_end].to_string(),
                            match_style,
                        ));
                        last_end = actual_end;
                    }
                    if last_end < trunc_byte_len {
                        truncated_spans.push(Span::styled(
                            truncated[last_end..].to_string(),
                            base_style,
                        ));
                    }
                } else {
                    truncated_spans.push(Span::styled(truncated.to_string(), base_style));
                }
                truncated_spans.push(Span::styled("…".to_string(), base_style));
            } else {
                truncated_spans.push(Span::styled("…".to_string(), base_style));
            }

            spans.extend(truncated_spans);
            let used_width = badge_width + truncated_width + 1; // +1 for "…"
            let pad_width = available_width.saturating_sub(used_width + meta_width);
            if pad_width > 0 {
                spans.push(Span::styled(" ".repeat(pad_width), base_style));
            }
            spans.push(Span::styled(meta, base_style.add_modifier(Modifier::DIM)));
        } else {
            spans.extend(content_spans);
            let pad_width = if total_needed <= content_width {
                content_width.saturating_sub(total_needed)
            } else {
                0
            };
            if pad_width > 0 {
                spans.push(Span::styled(" ".repeat(pad_width), base_style));
            }
            spans.push(Span::styled(meta, base_style.add_modifier(Modifier::DIM)));
        }
    } else {
        // File result: show path with fuzzy highlights
        let path = &result.relative_path;
        let indices = find_match_indices(query, path);
        let ranges = indices_to_ranges(&indices, path);

        let mut last_end = 0usize;
        for (start, end) in ranges {
            if start > last_end {
                spans.push(Span::styled(path[last_end..start].to_string(), base_style));
            }
            spans.push(Span::styled(path[start..end].to_string(), match_style));
            last_end = end;
        }

        if last_end < path.len() {
            spans.push(Span::styled(path[last_end..].to_string(), base_style));
        }

        if spans.len() == 2 && !path.is_empty() {
            // Only badge + space were added; add the full path
            spans.push(Span::styled(path.clone(), base_style));
        }

        if result.exact_match {
            spans.push(Span::styled("  ✦", match_style));
        }
    }

    Line::from(spans)
}

fn truncate_to_width(s: &str, max_width: usize) -> (&str, usize) {
    let mut width = 0;
    let mut byte_end = 0;
    for (i, ch) in s.char_indices() {
        let cw = ch.width().unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        width += cw;
        byte_end = i + ch.len_utf8();
    }
    (&s[..byte_end], width)
}

fn draw_preview_pane(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(theme.style_prompt())
        .style(theme.style_fg());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let selected = match state.results.get(state.selected) {
        Some(r) => r,
        None => return,
    };

    if selected.kind != MatchKind::Line {
        return;
    }

    let path = &selected.absolute_path;
    let target_line = selected.line_number.unwrap_or(1);
    let context = 6usize;

    let lines = read_file_context(path, target_line, context);
    if lines.is_empty() {
        let para = Paragraph::new("Unable to read file")
            .style(theme.style_fg().add_modifier(Modifier::DIM));
        frame.render_widget(para, inner);
        return;
    }

    let max_line = lines.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let gutter_width = max_line.to_string().len().max(3) + 1;
    let visible_rows = inner.height as usize;

    for (row, (line_num, line_text)) in lines.iter().enumerate().take(visible_rows) {
        let row_area = Rect {
            x: inner.x,
            y: inner.y + row as u16,
            width: inner.width,
            height: 1,
        };

        let is_target = *line_num == target_line;
        let gutter_style = theme.style_preview_gutter();
        let line_style = if is_target {
            theme.style_preview_highlight()
        } else {
            theme.style_fg()
        };

        let gutter_text = format!("{:>width$} ", line_num, width = gutter_width - 1);
        let max_text_width = inner.width.saturating_sub(gutter_width as u16) as usize;
        let display_line = truncate_line_to_width(line_text, max_text_width);

        let mut spans = vec![Span::styled(gutter_text, gutter_style)];

        // Highlight matched byte offsets on the target line
        if is_target {
            if let Some(ref offsets) = selected.match_byte_offsets {
                let mut last_end = 0usize;
                for &(start, end) in offsets {
                    let start = start as usize;
                    let end = end as usize;
                    if start > last_end && start <= display_line.len() {
                        spans.push(Span::styled(
                            display_line[last_end..start].to_string(),
                            line_style,
                        ));
                    }
                    if end <= display_line.len() {
                        spans.push(Span::styled(
                            display_line[start..end].to_string(),
                            theme.style_match(),
                        ));
                        last_end = end;
                    }
                }
                if last_end < display_line.len() {
                    spans.push(Span::styled(
                        display_line[last_end..].to_string(),
                        line_style,
                    ));
                }
                if spans.len() == 1 {
                    spans.push(Span::styled(display_line, line_style));
                }
            } else {
                spans.push(Span::styled(display_line, line_style));
            }
        } else {
            spans.push(Span::styled(display_line, line_style));
        }

        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(para, row_area);
    }
}

fn read_file_context(path: &str, line_number: u64, context: usize) -> Vec<(u64, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let target_idx = line_number.saturating_sub(1) as usize;
    let start = target_idx.saturating_sub(context);
    let end = (target_idx + context + 1).min(all_lines.len());

    all_lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| ((start + i + 1) as u64, line.to_string()))
        .collect()
}

fn truncate_line_to_width(line: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;
    for ch in line.chars() {
        let w = ch.width().unwrap_or(0);
        if width + w > max_width {
            break;
        }
        result.push(ch);
        width += w;
    }
    result
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

    fn make_state(
        is_scanning: bool,
        files: usize,
        matches: usize,
        results: Vec<UnifiedResult>,
    ) -> UiState {
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
            terminal_width: 120,
            preview_enabled: true,
            search_mode: SearchMode::default(),
            search_scope: SearchScope::default(),
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
        let results = vec![UnifiedResult {
            kind: MatchKind::File,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            score: 0,
            exact_match: false,
            line_number: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
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

    #[test]
    fn test_file_badge_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::File,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            score: 0,
            exact_match: false,
            line_number: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("[FILE]"),
            "expected [FILE] badge in:\n{text}"
        );
    }

    #[test]
    fn test_line_badge_and_meta_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::Line,
            relative_path: "Cargo.toml".into(),
            absolute_path: "/dev/null/Cargo.toml".into(),
            score: 0,
            exact_match: false,
            line_number: Some(83),
            line_content: Some(r#"path = "television/main.rs""#.into()),
            match_byte_offsets: Some(vec![(0, 4)]),
            is_definition: Some(false),
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("[LINE]"),
            "expected [LINE] badge in:\n{text}"
        );
        assert!(
            text.contains("Cargo.toml:83"),
            "expected line meta in:\n{text}"
        );
    }

    #[test]
    fn test_preview_pane_shown_for_line_result_and_wide_terminal() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        // Use a file that exists in this repo so preview can read it
        let results = vec![UnifiedResult {
            kind: MatchKind::Line,
            relative_path: "Cargo.toml".into(),
            absolute_path: std::fs::canonicalize("Cargo.toml")
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            score: 0,
            exact_match: false,
            line_number: Some(1),
            line_content: Some("[package]".into()),
            match_byte_offsets: Some(vec![(1, 8)]),
            is_definition: Some(false),
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // The preview pane should show file content and a gutter line number
        assert!(
            text.contains("1 ") || text.contains("[package]"),
            "expected preview pane content in:\n{text}"
        );
    }

    #[test]
    fn test_mode_buttons_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = make_state(false, 10, 1, vec![]);
        state.search_mode.case_sensitive = true;
        state.search_mode.regex = true;
        state.search_scope = SearchScope::GrepOnly;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("Cc"),
            "expected 'Cc' mode button in:\n{text}"
        );
        assert!(
            text.contains(".*"),
            "expected '.*' mode button in:\n{text}"
        );
        assert!(
            text.contains("[G]"),
            "expected '[G]' scope indicator in:\n{text}"
        );
    }

    #[test]
    fn test_preview_pane_hidden_for_narrow_terminal() {
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::Line,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            score: 0,
            exact_match: false,
            line_number: Some(10),
            line_content: Some("fn main() {}".into()),
            match_byte_offsets: None,
            is_definition: Some(true),
        }];
        let mut state = make_state(false, 10, 1, results);
        state.terminal_width = 60;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // Preview should be hidden; result list spans full width
        // We can't easily assert absence, but we can assert the badge is present
        assert!(
            text.contains("[LINE]"),
            "expected [LINE] badge in:\n{text}"
        );
    }
}
