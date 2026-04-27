use crate::highlight::{find_match_indices, highlight_content, indices_to_ranges};
use crate::icons;
use crate::picker::{selection_key, MatchKind, SearchMode, SearchScope, UnifiedResult};
use crate::theme::Theme;
use std::collections::HashSet;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
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
    pub selected_keys: HashSet<String>,
    pub is_scanning: bool,
    pub spinner_frame: usize,
    pub terminal_width: u16,
    pub preview_enabled: bool,
    pub search_mode: SearchMode,
    pub search_scope: SearchScope,
    pub group_grep: bool,
    pub path_shorten_strategy: String,
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
            "No files found"
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

        let is_marked = state.selected_keys.contains(&selection_key(result));
        let line = build_result_line(result, &state.highlight_query, theme, inner.width as usize, is_selected, is_marked, state.group_grep, &state.path_shorten_strategy);
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
    is_marked: bool,
    group_grep: bool,
    path_shorten_strategy: &str,
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

    // Helper: apply selected background to any style when row is selected
    let sel = |s: Style| -> Style {
        if is_selected { s.bg(theme.selected_bg) } else { s }
    };

    let mut spans: Vec<Span<'static>> = Vec::new();

    // Selection pointer + multi-select marker + git badge
    let pointer = if is_selected { ">" } else { " " };
    let marker = if is_marked { "▊" } else { " " };
    let (git_char, git_color) = if result.kind == MatchKind::Line {
        (' ', theme.fg)
    } else {
        git_badge(result.git_status.as_deref(), theme)
    };
    let pointer_style = if is_selected {
        sel(Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD))
    } else {
        base_style
    };
    let marker_style = if is_marked {
        sel(Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD))
    } else {
        base_style
    };
    spans.push(Span::styled(pointer.to_string(), pointer_style));
    spans.push(Span::styled(marker.to_string(), marker_style));
    spans.push(Span::styled(
        format!("{git_char} "),
        sel(Style::default().fg(git_color).add_modifier(Modifier::BOLD)),
    ));

    let prefix_width = 4;
    let content_width = available_width.saturating_sub(prefix_width);

    if result.kind == MatchKind::Line {
        let (content, offset_shift) = strip_leading_whitespace(result.line_content.as_deref().unwrap_or(""));

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

        if group_grep {
            // Grouped mode: :line:col gutter on the left, indented under file header
            let line_num = result.line_number.unwrap_or(0);
            let col = result.column.unwrap_or(1);
            let gutter = format!("  :{line_num}:{col} ");
            let gutter_width = gutter.width();

            spans.push(Span::styled(gutter, theme.style_dim()));

            let remaining_width = content_width.saturating_sub(gutter_width);
            let content_display_width: usize = content_spans.iter().map(|s| s.content.width()).sum();

            if content_display_width > remaining_width && remaining_width > 3 {
                let max_content_display = remaining_width.saturating_sub(1);
                let (truncated, _) = truncate_to_width(content, max_content_display);
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
            } else {
                spans.extend(content_spans);
            }
        } else {
            // Non-grouped mode: content with filename:line meta on the right
            let file_name = std::path::Path::new(&result.relative_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&result.relative_path);
            let line_num = result.line_number.unwrap_or(0);
            let meta = format!("{file_name}:{line_num}");
            let meta_width = meta.width();
            let gap = 2;

            let content_display_width: usize = content_spans.iter().map(|s| s.content.width()).sum();
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
                spans.push(Span::styled(meta, sel(theme.style_dim())));
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
                spans.push(Span::styled(meta, sel(theme.style_dim())));
            }
        }
    } else if result.kind == MatchKind::File {
        // File result: icon + filename (highlighted) + dimmed directory
        let path = &result.relative_path;
        let icon_info = icons::lookup(path);
        let icon_str = icon_info.map(|i| i.icon).unwrap_or(" ");
        let icon_color = icon_info.map(|i| i.color).unwrap_or(theme.dim_fg);

        spans.push(Span::styled(
            format!("{icon_str} "),
            sel(Style::default().fg(icon_color)),
        ));

        let path_obj = std::path::Path::new(path);
        let filename = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or(path);
        let dir_path = path_obj
            .parent()
            .and_then(|p| p.to_str())
            .filter(|d| !d.is_empty() && *d != ".")
            .unwrap_or("");

        let prefix_width = 4 + icon_str.width() + 1; // pointer + marker + git-col + icon + space
        let content_width = available_width.saturating_sub(prefix_width);

        // Build filename spans with fuzzy highlights
        let mut file_spans: Vec<Span<'static>> = Vec::new();
        let filename_ranges = indices_to_ranges(&find_match_indices(query, filename), filename);
        let mut last_end = 0usize;
        for (start, end) in &filename_ranges {
            if *start > last_end {
                file_spans.push(Span::styled(filename[last_end..*start].to_string(), base_style));
            }
            file_spans.push(Span::styled(filename[*start..*end].to_string(), match_style));
            last_end = *end;
        }
        if last_end < filename.len() {
            file_spans.push(Span::styled(filename[last_end..].to_string(), base_style));
        }
        if file_spans.is_empty() && !filename.is_empty() {
            file_spans.push(Span::styled(filename.to_string(), base_style));
        }
        let file_width: usize = file_spans.iter().map(|s| s.content.width()).sum();

        // Build dimmed directory spans
        let mut dir_spans: Vec<Span<'static>> = Vec::new();
        if !dir_path.is_empty() {
            let dir_text = format!(" {dir_path}");
            let dir_width = dir_text.width();
            if file_width + dir_width <= content_width {
                dir_spans.push(Span::styled(dir_text, sel(theme.style_dim())));
            } else if content_width > file_width + 4 {
                let max_dir = content_width.saturating_sub(file_width + 2);
                let shortened = shorten_dir_path(dir_path, max_dir, path_shorten_strategy);
                if !shortened.is_empty() {
                    dir_spans.push(Span::styled(format!(" {shortened}"), sel(theme.style_dim())));
                }
            }
        }
        let dir_width: usize = dir_spans.iter().map(|s| s.content.width()).sum();

        let total_used = prefix_width + file_width + dir_width;
        let pad_width = available_width.saturating_sub(total_used);

        spans.extend(file_spans);
        spans.extend(dir_spans);
        if pad_width > 0 {
            spans.push(Span::styled(" ".repeat(pad_width), base_style));
        }

        if result.exact_match {
            spans.push(Span::styled("✦", match_style));
        }
    } else {
        // FileHeader result: bold icon + filename + dimmed directory (grouped grep header)
        let path = &result.relative_path;
        let icon_info = icons::lookup(path);
        let icon_str = icon_info.map(|i| i.icon).unwrap_or(" ");
        let icon_color = icon_info.map(|i| i.color).unwrap_or(theme.dim_fg);

        spans.push(Span::styled(
            format!("{icon_str} "),
            sel(Style::default().fg(icon_color).add_modifier(Modifier::BOLD)),
        ));

        let path_obj = std::path::Path::new(path);
        let filename = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or(path);
        let dir_path = path_obj
            .parent()
            .and_then(|p| p.to_str())
            .filter(|d| !d.is_empty() && *d != ".")
            .unwrap_or("");

        let prefix_width = 4 + icon_str.width() + 1;
        let content_width = available_width.saturating_sub(prefix_width);

        let mut file_spans: Vec<Span<'static>> = Vec::new();
        let filename_ranges = indices_to_ranges(&find_match_indices(query, filename), filename);
        let mut last_end = 0usize;
        let header_base = base_style.add_modifier(Modifier::BOLD);
        let header_match = match_style.add_modifier(Modifier::BOLD);
        for (start, end) in &filename_ranges {
            if *start > last_end {
                file_spans.push(Span::styled(filename[last_end..*start].to_string(), header_base));
            }
            file_spans.push(Span::styled(filename[*start..*end].to_string(), header_match));
            last_end = *end;
        }
        if last_end < filename.len() {
            file_spans.push(Span::styled(filename[last_end..].to_string(), header_base));
        }
        if file_spans.is_empty() && !filename.is_empty() {
            file_spans.push(Span::styled(filename.to_string(), header_base));
        }
        let file_width: usize = file_spans.iter().map(|s| s.content.width()).sum();

        let mut dir_spans: Vec<Span<'static>> = Vec::new();
        if !dir_path.is_empty() {
            let dir_text = format!(" {dir_path}");
            let dir_width = dir_text.width();
            if file_width + dir_width <= content_width {
                dir_spans.push(Span::styled(dir_text, sel(theme.style_dim().add_modifier(Modifier::BOLD))));
            } else if content_width > file_width + 4 {
                let max_dir = content_width.saturating_sub(file_width + 2);
                let shortened = shorten_dir_path(dir_path, max_dir, path_shorten_strategy);
                if !shortened.is_empty() {
                    dir_spans.push(Span::styled(format!(" {shortened}"), sel(theme.style_dim().add_modifier(Modifier::BOLD))));
                }
            }
        }
        let dir_width: usize = dir_spans.iter().map(|s| s.content.width()).sum();

        let total_used = prefix_width + file_width + dir_width;
        let pad_width = available_width.saturating_sub(total_used);

        spans.extend(file_spans);
        spans.extend(dir_spans);
        if pad_width > 0 {
            spans.push(Span::styled(" ".repeat(pad_width), base_style));
        }
    }

    Line::from(spans)
}

fn git_badge(status: Option<&str>, theme: &Theme) -> (char, Color) {
    match status {
        Some("modified") => ('M', theme.git_modified_fg),
        Some("staged_new") | Some("staged_modified") => ('A', theme.git_added_fg),
        Some("untracked") => ('?', theme.git_untracked_fg),
        Some("deleted") => ('D', theme.git_deleted_fg),
        Some("renamed") => ('R', theme.git_renamed_fg),
        Some("ignored") => ('I', theme.git_ignored_fg),
        _ => (' ', theme.fg),
    }
}

fn shorten_dir_path(dir_path: &str, max_width: usize, strategy: &str) -> String {
    if dir_path.is_empty() || dir_path.width() <= max_width {
        return dir_path.to_string();
    }

    let components: Vec<&str> = dir_path.split('/').collect();
    if components.len() <= 2 {
        let (truncated, _) = truncate_to_width(dir_path, max_width.saturating_sub(1));
        if !truncated.is_empty() {
            return format!("{truncated}…");
        }
        return dir_path.to_string();
    }

    let first = components[0];
    let last = components[components.len() - 1];

    match strategy {
        "middle" => {
            let shortened = format!("{first}/.../{last}");
            if shortened.width() <= max_width {
                return shortened;
            }
            let alt = format!(".../{last}");
            if alt.width() <= max_width {
                return alt;
            }
        }
        "middle_number" => {
            let hidden = components.len().saturating_sub(2);
            let middle = if hidden <= 3 { "..." } else { &format!(".{hidden}.") };
            let shortened = format!("{first}/{middle}/{last}");
            if shortened.width() <= max_width {
                return shortened;
            }
            let alt = format!(".../{last}");
            if alt.width() <= max_width {
                return alt;
            }
        }
        "end" => {
            let (truncated, _) = truncate_to_width(dir_path, max_width.saturating_sub(1));
            if !truncated.is_empty() {
                return format!("{truncated}…");
            }
        }
        _ => {}
    }

    // Fallback: just truncate
    let (truncated, _) = truncate_to_width(dir_path, max_width.saturating_sub(1));
    if !truncated.is_empty() {
        format!("{truncated}…")
    } else {
        dir_path.to_string()
    }
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
        Span::styled(": quit ", theme.style_status()),
        Span::styled("tab", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": sel ", theme.style_status()),
        Span::styled("shift-tab", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": mode ", theme.style_status()),
        Span::styled("↑↓", Style::default().fg(theme.match_fg).add_modifier(Modifier::BOLD)),
        Span::styled(": nav", theme.style_status()),
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

    if state.group_grep {
        hints.push(Span::styled("  [grouped]", theme.style_dim()));
    }

    frame.render_widget(
        Paragraph::new(Line::from(hints)).alignment(Alignment::Center),
        chunks[1],
    );

    // Right: match count + grep mode + version
    let mode_label = if state.search_scope != SearchScope::FileOnly {
        match (state.search_mode.regex, state.search_mode.fuzzy) {
            (true, _) => " regex",
            (_, true) => " fuzzy",
            _ => " plain",
        }
    } else {
        ""
    };
    let right_spans = vec![Span::styled(
        format!("{} matches{}  v0.1.0 ", state.total_matched, mode_label),
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
            selected_keys: HashSet::new(),
            is_scanning,
            spinner_frame: 0,
            terminal_width: 120,
            preview_enabled: true,
            search_mode: SearchMode::default(),
            search_scope: SearchScope::default(),
            group_grep: false,
            path_shorten_strategy: "middle_number".into(),
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
            column: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
            git_status: None,
        }];
        let state = make_state(false, 42, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("Results"),
            "expected results title in:\n{text}"
        );
        assert!(
            text.contains("main.rs"),
            "expected result row in:\n{text}"
        );
    }

    #[test]
    fn test_file_icon_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::File,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            score: 0,
            exact_match: false,
            line_number: None,
            column: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
            git_status: None,
        }];
        let state = make_state(false, 42, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // Rust icon from nvim-web-devicons
        assert!(
            text.contains(''),
            "expected nerd-font icon for .rs file in:\n{text}"
        );
        assert!(
            text.contains("main.rs"),
            "expected filename in:\n{text}"
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
            column: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
            git_status: None,
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
            column: Some(1),
            line_content: Some(r#"path = "television/main.rs""#.into()),
            match_byte_offsets: Some(vec![(0, 4)]),
            is_definition: Some(false),
            git_status: None,
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // Non-grouped mode shows filename:line on the right
        assert!(
            text.contains("Cargo.toml:83"),
            "expected right-side meta in non-grouped mode:\n{text}"
        );
    }

    #[test]
    fn test_grouped_line_result_left_gutter() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::Line,
            relative_path: "Cargo.toml".into(),
            absolute_path: "/dev/null/Cargo.toml".into(),
            score: 0,
            exact_match: false,
            line_number: Some(83),
            column: Some(1),
            line_content: Some(r#"path = "television/main.rs""#.into()),
            match_byte_offsets: Some(vec![(0, 4)]),
            is_definition: Some(false),
            git_status: None,
        }];
        let mut state = make_state(false, 10, 1, results);
        state.group_grep = true;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // Grouped mode shows :line:col on the left
        assert!(
            text.contains(":83:1"),
            "expected left gutter :line:col in grouped mode:\n{text}"
        );
        // Right-side meta should not appear
        assert!(
            !text.contains("Cargo.toml:83"),
            "expected no right-side meta in grouped mode:\n{text}"
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
            column: Some(2),
            line_content: Some("[package]".into()),
            match_byte_offsets: Some(vec![(1, 8)]),
            is_definition: Some(false),
            git_status: None,
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
            column: Some(1),
            line_content: Some("fn main() {}".into()),
            match_byte_offsets: None,
            is_definition: Some(true),
            git_status: None,
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

    #[test]
    fn test_file_header_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::FileHeader,
            relative_path: "src/app.rs".into(),
            absolute_path: "/dev/null/src/app.rs".into(),
            score: 0,
            exact_match: false,
            line_number: None,
            column: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
            git_status: None,
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("app.rs"),
            "expected file header filename in:\n{text}"
        );
    }

    #[test]
    fn test_grouped_grep_renders_short_meta() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![
            UnifiedResult {
                kind: MatchKind::FileHeader,
                relative_path: "src/app.rs".into(),
                absolute_path: "/dev/null/src/app.rs".into(),
                score: 0,
                exact_match: false,
                line_number: None,
                column: None,
                line_content: None,
                match_byte_offsets: None,
                is_definition: None,
                git_status: Some("modified".into()),
            },
            UnifiedResult {
                kind: MatchKind::Line,
                relative_path: "src/app.rs".into(),
                absolute_path: "/dev/null/src/app.rs".into(),
                score: 0,
                exact_match: false,
                line_number: Some(42),
                column: Some(5),
                line_content: Some("pub struct App {}".into()),
                match_byte_offsets: Some(vec![(4, 7)]),
                is_definition: Some(true),
                git_status: Some("modified".into()),
            },
        ];
        let mut state = make_state(false, 10, 2, results);
        state.group_grep = true;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("app.rs"),
            "expected file header in:\n{text}"
        );
        assert!(
            text.contains(":42:5"),
            "expected left gutter :line:col in:\n{text}"
        );
        assert!(
            text.contains("pub struct"),
            "expected line content in:\n{text}"
        );
        // Old right-side combined meta should not appear
        assert!(
            !text.contains("app.rs:42"),
            "expected no right-side combined meta:\n{text}"
        );
        // FileHeader should show git badge, but line should not
        let lines: Vec<&str> = text.lines().collect();
        let header_line = lines.iter().find(|l| l.contains("app.rs") && l.contains('M')).expect("header with M");
        let line_row = lines.iter().find(|l| l.contains(":42:5")).expect("line row");
        assert!(
            header_line.contains('M'),
            "expected FileHeader to show git badge:\n{text}"
        );
        assert!(
            !line_row.contains('M'),
            "expected Line result to NOT show git badge:\n{text}"
        );
    }

    #[test]
    fn test_grouped_indicator_in_status_bar() {
        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![];
        let mut state = make_state(false, 10, 0, results);
        state.group_grep = true;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("[grouped]"),
            "expected [grouped] indicator in status bar:\n{text}"
        );
    }

    #[test]
    fn test_multi_select_marker_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![
            UnifiedResult {
                kind: MatchKind::File,
                relative_path: "src/main.rs".into(),
                absolute_path: "/dev/null/src/main.rs".into(),
                score: 0,
                exact_match: false,
                line_number: None,
                column: None,
                line_content: None,
                match_byte_offsets: None,
                is_definition: None,
            git_status: None,
            },
            UnifiedResult {
                kind: MatchKind::File,
                relative_path: "src/lib.rs".into(),
                absolute_path: "/dev/null/src/lib.rs".into(),
                score: 0,
                exact_match: false,
                line_number: None,
                column: None,
                line_content: None,
                match_byte_offsets: None,
                is_definition: None,
            git_status: None,
            },
        ];
        let mut state = make_state(false, 10, 2, results);
        state.selected_keys.insert("/dev/null/src/lib.rs".into());

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        // First row is cursor, should have pointer
        assert!(
            text.contains("> "),
            "expected selection pointer in:\n{text}"
        );
        // Second row should have marker but no pointer
        assert!(
            text.contains("▊"),
            "expected multi-select marker in:\n{text}"
        );
    }

    #[test]
    fn test_grep_mode_indicator_in_status_bar() {
        let backend = TestBackend::new(120, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![];
        let mut state = make_state(false, 10, 0, results);
        state.search_mode.regex = true;

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains("regex"),
            "expected regex mode indicator in status bar:\n{text}"
        );
    }

    #[test]
    fn test_git_badge_rendered() {
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let results = vec![UnifiedResult {
            kind: MatchKind::File,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            git_status: Some("modified".into()),
            ..Default::default()
        }];
        let state = make_state(false, 10, 1, results);

        terminal.draw(|f| draw(f, &state, &Theme::default())).unwrap();

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(
            text.contains('M'),
            "expected git modified badge 'M' in:\n{text}"
        );
    }

    #[test]
    fn test_shorten_dir_path_middle_number() {
        // 5 components, 3 hidden -> "..." because hidden <= 3
        assert_eq!(
            shorten_dir_path("a/b/c/d/e", 8, "middle_number"),
            "a/.../e"
        );
        // 6 components, 4 hidden -> ".4." because hidden > 3
        assert_eq!(
            shorten_dir_path("a/b/c/d/e/f", 8, "middle_number"),
            "a/.4./f"
        );
        // Short path that already fits
        assert_eq!(
            shorten_dir_path("a/b", 10, "middle_number"),
            "a/b"
        );
    }

    #[test]
    fn test_shorten_dir_path_middle() {
        assert_eq!(
            shorten_dir_path("a/b/c/d/e", 8, "middle"),
            "a/.../e"
        );
    }

    #[test]
    fn test_shorten_dir_path_end() {
        let result = shorten_dir_path("very/long/path/name", 10, "end");
        assert!(result.ends_with('…'), "expected truncation with ellipsis: {result}");
    }
}
