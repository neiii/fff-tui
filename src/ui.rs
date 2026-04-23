use crate::highlight::{find_match_indices, highlight_content, indices_to_ranges};
use crate::picker::{MatchKind, SearchMode, SearchScope, UnifiedResult};
use crate::theme::Theme;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Padding as RatatuiPadding, Paragraph},
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

    // Reserve 1 line at the very bottom for the status bar (full width)
    let status_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(1),
        width: area.width,
        height: 1,
    };
    let work_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height.saturating_sub(1),
    };

    let show_preview = state.preview_enabled
        && state.terminal_width >= 70
        && state.results.get(state.selected).is_some();

    let (left_area, preview_area) = if show_preview {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(work_area);
        (hchunks[0], Some(hchunks[1]))
    } else {
        (work_area, None)
    };

    // Stack input bar + results inside the left column
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // input bar
            Constraint::Min(1),    // results list
        ])
        .split(left_area);

    draw_input_box(frame, left_chunks[0], state, theme);
    draw_result_list(frame, left_chunks[1], state, theme);
    if let Some(preview_rect) = preview_area {
        draw_preview_pane(frame, preview_rect, state, theme);
    }
    draw_status_bar(frame, status_area, state, theme);
}

fn draw_input_box(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let title = if state.is_scanning {
        " fff • scanning… "
    } else {
        " fff "
    };

    let block = Block::default()
        .title(
            Line::from(title)
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.mode_bg).add_modifier(Modifier::BOLD)),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_border())
        .style(theme.style_fg());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.area() == 0 {
        return;
    }

    // Split into prompt, input, and counter
    let total_digits = state.total_matched.max(1).to_string().len() as u16;
    let count_width = 3 * total_digits + 3; // " N / M "

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2), // "> "
            Constraint::Fill(1),
            Constraint::Length(count_width),
        ])
        .split(inner);

    let prompt = Paragraph::new(Span::styled(
        "> ",
        Style::default().fg(theme.prompt_fg).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(prompt, chunks[0]);

    let input = Paragraph::new(state.query.clone())
        .style(Style::default().fg(theme.fg).add_modifier(Modifier::BOLD).italic());
    frame.render_widget(input, chunks[1]);

    let count_text = if state.results.is_empty() {
        " 0 / 0 ".to_string()
    } else {
        format!(
            " {} / {} ",
            state.selected + 1,
            state.results.len()
        )
    };
    let counter = Paragraph::new(Span::styled(
        count_text,
        Style::default().fg(theme.prompt_fg).italic(),
    ))
    .alignment(Alignment::Right);
    frame.render_widget(counter, chunks[2]);

    // Cursor position
    let cursor_x = chunks[1].x + state.query.width() as u16;
    if cursor_x < chunks[1].x + chunks[1].width {
        frame.set_cursor_position((cursor_x, chunks[1].y));
    }
}

fn draw_result_list(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let block = Block::default()
        .title(
            Line::from(" Results ")
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.border_fg)),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_border())
        .style(theme.style_fg())
        .padding(RatatuiPadding::uniform(0));

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
            .style(theme.style_dim())
            .alignment(Alignment::Center);
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

        let line = build_result_line(result, &state.highlight_query, theme, inner.width as usize, is_selected);
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

    // Selection pointer
    if is_selected {
        spans.push(Span::styled("> ", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)));
    } else {
        spans.push(Span::styled("  ", base_style));
    }

    let prefix_width = 2;
    let content_width = available_width.saturating_sub(prefix_width);

    if result.kind == MatchKind::Line {
        let (content, offset_shift) = strip_leading_whitespace(result.line_content.as_deref().unwrap_or(""));
        let file_name = std::path::Path::new(&result.relative_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&result.relative_path);
        let line_num = result.line_number.unwrap_or(0);
        let meta = format!("{file_name}:{line_num}");
        let meta_width = meta.width();
        let gap = 2;

        // Adjust byte offsets for stripped leading whitespace
        let adjusted_offsets: Option<Vec<(u32, u32)>> = result.match_byte_offsets.as_ref().map(|offsets| {
            offsets
                .iter()
                .filter_map(|&(start, end)| {
                    let s = start.saturating_sub(offset_shift as u32);
                    let e = end.saturating_sub(offset_shift as u32);
                    if e > 0 && s < content.len() as u32 {
                        Some((s, e.min(content.len() as u32)))
                    } else {
                        None
                    }
                })
                .collect()
        });

        // Build highlighted content spans
        let mut content_spans: Vec<Span<'static>> = Vec::new();
        if let Some(ref offsets) = adjusted_offsets {
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
            let max_content_display = content_width.saturating_sub(meta_width + gap + 1);
            let (truncated, truncated_width) = truncate_to_width(content, max_content_display);
            let mut truncated_spans = Vec::new();
            if !truncated.is_empty() {
                let trunc_byte_len = truncated.len();
                if let Some(ref offsets) = adjusted_offsets {
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
            let used_width = prefix_width + truncated_width + 1;
            let pad_width = available_width.saturating_sub(used_width + meta_width);
            if pad_width > 0 {
                spans.push(Span::styled(" ".repeat(pad_width), base_style));
            }
            spans.push(Span::styled(meta, theme.style_dim()));
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
            spans.push(Span::styled(meta, theme.style_dim()));
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

        if spans.len() == 1 && !path.is_empty() {
            // Only pointer was added; add the full path
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

/// Strip leading whitespace from a string and return the number of bytes stripped.
fn strip_leading_whitespace(s: &str) -> (&str, usize) {
    let trimmed = s.trim_start();
    let shift = s.len() - trimmed.len();
    (trimmed, shift)
}

fn draw_preview_pane(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let selected = match state.results.get(state.selected) {
        Some(r) => r,
        None => return,
    };

    let title = &selected.relative_path;

    let block = Block::default()
        .title(
            Line::from(format!(" {title} "))
                .alignment(Alignment::Center)
                .style(Style::default().fg(theme.preview_title_fg).add_modifier(Modifier::BOLD)),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.style_border())
        .style(theme.style_fg())
        .padding(RatatuiPadding::uniform(0));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let path = &selected.absolute_path;

    let (start_line, target_line, line_count) = if selected.kind == MatchKind::Line {
        let target = selected.line_number.unwrap_or(1);
        let ctx = (inner.height as usize).saturating_sub(1) / 2;
        let start = target.saturating_sub(ctx as u64).max(1);
        (start, Some(target), inner.height as usize)
    } else {
        (1, None, inner.height as usize)
    };

    let lines = read_file_lines(path, start_line, line_count);
    if lines.is_empty() {
        let para = Paragraph::new("Unable to read file")
            .style(theme.style_dim());
        frame.render_widget(para, inner);
        return;
    }

    // Try to read enough content for syntax highlighting
    let highlight_text = lines
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let highlighted = highlight_content(path, &highlight_text);

    let gutter_width = lines
        .iter()
        .map(|(n, _)| n.to_string().len())
        .max()
        .unwrap_or(3)
        .max(3)
        + 1;

    let text_width = inner.width.saturating_sub(gutter_width as u16) as usize;

    for (row, (line_num, line_text)) in lines.iter().enumerate().take(inner.height as usize) {
        let row_area = Rect {
            x: inner.x,
            y: inner.y + row as u16,
            width: inner.width,
            height: 1,
        };

        let is_target = target_line.map(|t| *line_num == t).unwrap_or(false);
        let gutter_style = theme.style_preview_gutter();

        let gutter_text = format!("{:>width$} ", line_num, width = gutter_width - 1);
        let display_line = truncate_line_to_width(line_text, text_width);

        let mut spans = vec![Span::styled(gutter_text, gutter_style)];

        // Get highlighted spans for this line if available
        if let Some(hl_line) = highlighted.lines.get(row) {
            let hl_text: String = hl_line.spans.iter().map(|s| s.content.as_ref()).collect();
            if hl_text.trim() == display_line.trim() {
                // Use the highlighted spans directly, but truncate to fit
                let mut remaining_width = text_width;
                for span in &hl_line.spans {
                    let span_text = span.content.as_ref();
                    let span_width = span_text.width();
                    if remaining_width == 0 {
                        break;
                    }
                    if span_width <= remaining_width {
                        spans.push(span.clone());
                        remaining_width -= span_width;
                    } else {
                        let truncated = truncate_line_to_width(span_text, remaining_width);
                        spans.push(Span::styled(truncated, span.style));
                        remaining_width = 0;
                    }
                }
            } else {
                spans.push(Span::styled(display_line.clone(), theme.style_fg()));
            }
        } else {
            spans.push(Span::styled(display_line.clone(), theme.style_fg()));
        }

        // Apply target line highlight
        if is_target {
            for span in &mut spans {
                span.style = span.style.bg(theme.preview_highlight_bg);
            }
        }

        let para = Paragraph::new(Line::from(spans));
        frame.render_widget(para, row_area);
    }
}

fn read_file_lines(path: &str, start_line: u64, count: usize) -> Vec<(u64, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let start_idx = start_line.saturating_sub(1) as usize;
    let end = (start_idx + count).min(all_lines.len());

    all_lines[start_idx..end]
        .iter()
        .enumerate()
        .map(|(i, line)| ((start_idx + i + 1) as u64, line.to_string()))
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

fn draw_status_bar(frame: &mut Frame, area: Rect, state: &UiState, theme: &Theme) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),  // left: mode bubble + info
            Constraint::Fill(2),  // middle: hints
            Constraint::Fill(1),  // right: version / stats
        ])
        .split(area);

    // Left: mode bubble
    let mut left_spans = vec![Span::raw(" ")];
    left_spans.push(Span::styled(
        " FFF ",
        theme.style_mode_active(),
    ));
    left_spans.push(Span::styled(
        format!("  {} files", state.total_files),
        theme.style_status(),
    ));

    frame.render_widget(
        Paragraph::new(Line::from(left_spans)).alignment(Alignment::Left),
        chunks[0],
    );

    // Middle: hints
    let mut hints = vec![
        Span::styled("ctrl-c", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": quit  ", theme.style_status()),
        Span::styled("tab", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": preview  ", theme.style_status()),
        Span::styled("↑↓", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": navigate", theme.style_status()),
    ];

    // Add scope indicator
    let scope_label = match state.search_scope {
        SearchScope::Unified => "",
        SearchScope::FileOnly => "  [files]",
        SearchScope::GrepOnly => "  [grep]",
    };
    if !scope_label.is_empty() {
        hints.push(Span::styled(scope_label, theme.style_dim()));
    }

    frame.render_widget(
        Paragraph::new(Line::from(hints)).alignment(Alignment::Center),
        chunks[1],
    );

    // Right: match count + version
    let right_spans = vec![Span::styled(
        format!("{} matches  v0.1.0 ", state.total_matched),
        theme.style_status(),
    )];
    frame.render_widget(
        Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right),
        chunks[2],
    );
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
            text.contains("scanning…"),
            "expected 'scanning…' in title:\n{text}"
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
            text.contains("Results"),
            "expected results title in:\n{text}"
        );
        assert!(
            text.contains("src/main.rs"),
            "expected result row in:\n{text}"
        );
    }

    #[test]
    fn test_selection_pointer_rendered() {
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
            text.contains("> "),
            "expected selection pointer in:\n{text}"
        );
    }

    #[test]
    fn test_line_result_and_meta_rendered() {
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
            text.contains("Cargo.toml:83"),
            "expected line meta in:\n{text}"
        );
    }

    #[test]
    fn test_preview_pane_shown_for_wide_terminal() {
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
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
        assert!(
            text.contains("Cargo.toml"),
            "expected preview title in:\n{text}"
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
        assert!(
            text.contains("Results"),
            "expected Results title in:\n{text}"
        );
    }
}
