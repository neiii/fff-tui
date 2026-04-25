use crate::picker::{selection_key, MatchKind, PickerBackend, SearchMode, SearchScope, UnifiedResult};
use crate::theme::Theme;
use crate::ui::{draw, UiState};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::time::{Duration, Instant};

pub struct App {
    pub query: String,
    pub highlight_query: String,
    pub results: Vec<UnifiedResult>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub total_files: usize,
    pub total_matched: usize,
    pub should_quit: bool,
    pub should_select: bool,
    pub selected_items: Vec<UnifiedResult>,
    pub spinner_frame: usize,
    pub last_spinner_tick: Instant,
    pub terminal_height: u16,
    pub terminal_width: u16,
    pub last_search_refresh: Instant,
    pub search_mode: SearchMode,
    pub search_scope: SearchScope,
    pub preview_enabled: bool,
    pub path_shorten_strategy: String,
    pub current_file: Option<String>,
    pub force_combo_boost: bool,
    pub query_history_offset: usize,
    // ── Infinite-scroll pagination state ──
    /// Raw result categories.  Kept separate so we can append new pages
    /// while preserving the unified stacking order.
    pub exact_files: Vec<UnifiedResult>,
    pub line_results: Vec<UnifiedResult>,
    pub other_files: Vec<UnifiedResult>,
    /// True total fuzzy matches (reported by backend for offset 0).
    pub fuzzy_total_matched: usize,
    /// Cumulative grep matches loaded across all pages.
    pub cumulative_grep_matched: usize,
    /// Next file offset for grep (0 = no more).
    pub grep_next_file_offset: usize,
    /// Backend batch size.
    pub page_size: usize,
    // ── Unified pagination (page replacement) ──
    /// Current page index when in Unified scope (0 = first page).
    pub unified_page_index: usize,
    /// Start offsets for each visited page: (fuzzy_offset, grep_file_offset).
    pub unified_page_offsets: Vec<(usize, usize)>,
}

impl App {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            highlight_query: String::new(),
            results: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            total_files: 0,
            total_matched: 0,
            should_quit: false,
            should_select: false,
            selected_items: Vec::new(),
            spinner_frame: 0,
            last_spinner_tick: Instant::now(),
            terminal_height: 24,
            terminal_width: 80,
            last_search_refresh: Instant::now(),
            search_mode: SearchMode::default(),
            search_scope: SearchScope::default(),
            preview_enabled: true,
            path_shorten_strategy: "middle_number".into(),
            current_file: None,
            force_combo_boost: false,
            query_history_offset: 0,
            exact_files: Vec::new(),
            line_results: Vec::new(),
            other_files: Vec::new(),
            fuzzy_total_matched: 0,
            cumulative_grep_matched: 0,
            grep_next_file_offset: 0,
            page_size: 500,
            unified_page_index: 0,
            unified_page_offsets: vec![(0, 0)],
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<crate::tui::Backend>,
        backend: &PickerBackend,
    ) -> anyhow::Result<Option<Vec<UnifiedResult>>> {
        let tick_rate = Duration::from_millis(50);
        let mut dump_frame = 0usize;

        // ── Open immediately (like Lua extension) ──
        self.refresh_search(backend);

        while !self.should_quit && !self.should_select {
            if self.last_spinner_tick.elapsed() > Duration::from_millis(80) {
                self.spinner_frame += 1;
                self.last_spinner_tick = Instant::now();
            }

            let is_scanning = backend.is_scanning();

            // Refresh search periodically while scanning so files appear
            // incrementally (matching Lua's monitor_scan_progress).
            if is_scanning {
                self.total_files = backend.total_files();
                if self.last_search_refresh.elapsed() > Duration::from_millis(200) {
                    self.refresh_search(backend);
                }
            }

            if let Ok(size) = terminal.size() {
                self.terminal_height = size.height;
                self.terminal_width = size.width;
            }

            let ui_state = UiState {
                query: self.query.clone(),
                highlight_query: self.highlight_query.clone(),
                results: self.results.clone(),
                selected: self.selected,
                scroll_offset: self.scroll_offset,
                total_files: self.total_files,
                total_matched: self.total_matched,
                selected_keys: self.selected_items.iter().map(selection_key).collect(),
                is_scanning,
                spinner_frame: self.spinner_frame,
                terminal_width: self.terminal_width,
                preview_enabled: self.preview_enabled,
                search_mode: self.search_mode,
                search_scope: self.search_scope,
                group_grep: self.search_mode.group_grep,
                path_shorten_strategy: self.path_shorten_strategy.clone(),
            };
            terminal.draw(|f| {
                draw(f, &ui_state, &Theme::default());
                crate::debug_dump::dump_buffer(&*f.buffer_mut(), dump_frame);
                dump_frame += 1;
            })?;

            if event::poll(tick_rate)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key, backend),
                    Event::Resize(width, height) => {
                        self.terminal_width = width;
                        self.terminal_height = height;
                        self.ensure_visible();
                    }
                    _ => {}
                }
            }
        }

        if self.should_select {
            if self.selected_items.is_empty() {
                Ok(self.results.get(self.selected).cloned().map(|r| vec![r]))
            } else {
                Ok(Some(self.selected_items.clone()))
            }
        } else {
            Ok(None)
        }
    }

    fn handle_key(&mut self, key: KeyEvent, backend: &PickerBackend) {
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    match c {
                        'c' => self.should_quit = true,
                        'n' => self.move_selection(1, backend),
                        'p' => self.move_selection(-1, backend),
                        'a' => self.select_all_visible(),
                        'd' => self.deselect_all(),
                        'o' => self.preview_enabled = !self.preview_enabled,
                        'u' => self.move_selection_page(-1),
                        'l' => {
                            self.query.clear();
                            self.refresh_search(backend);
                        }
                        't' => {
                            self.search_scope = match self.search_scope {
                                SearchScope::FileOnly => SearchScope::Unified,
                                SearchScope::Unified => SearchScope::GrepOnly,
                                SearchScope::GrepOnly => SearchScope::FileOnly,
                            };
                            self.refresh_search(backend);
                        }
                        'g' => {
                            self.search_mode.group_grep = !self.search_mode.group_grep;
                            self.refresh_search(backend);
                        }
                        _ => {}
                    }
                } else if key.modifiers.contains(KeyModifiers::ALT) {
                    match c.to_ascii_lowercase() {
                        'c' => {
                            self.search_mode.case_sensitive = !self.search_mode.case_sensitive;
                            self.refresh_search(backend);
                        }
                        'w' => {
                            self.search_mode.whole_word = !self.search_mode.whole_word;
                            self.refresh_search(backend);
                        }
                        'r' => {
                            self.search_mode.regex = !self.search_mode.regex;
                            if self.search_mode.regex {
                                self.search_mode.fixed_strings = false;
                            }
                            self.refresh_search(backend);
                        }
                        'f' => {
                            self.search_mode.fixed_strings = !self.search_mode.fixed_strings;
                            if self.search_mode.fixed_strings {
                                self.search_mode.regex = false;
                            }
                            self.refresh_search(backend);
                        }
                        _ => {}
                    }
                } else {
                    self.query.push(c);
                    self.refresh_search(backend);
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.refresh_search(backend);
            }
            KeyCode::Delete => {
                self.query.clear();
                self.refresh_search(backend);
            }
            KeyCode::Enter if !self.results.is_empty() => {
                self.should_select = true;
            }
            KeyCode::Tab => self.toggle_selection(),
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Up => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.cycle_query_history(-1, backend);
                } else {
                    self.move_selection(-1, backend);
                }
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.cycle_query_history(1, backend);
                } else {
                    self.move_selection(1, backend);
                }
            }
            KeyCode::BackTab => {
                self.cycle_grep_mode();
                self.refresh_search(backend);
            }
            KeyCode::PageUp => self.move_selection_page(-1),
            KeyCode::PageDown => self.move_selection_page(1),
            KeyCode::Home => {
                self.selected = 0;
                self.scroll_offset = 0;
            }
            KeyCode::End if !self.results.is_empty() => {
                self.selected = self.results.len() - 1;
                self.ensure_visible();
                self.maybe_load_more(backend);
            }
            _ => {}
        }
    }

    // ── Search & Pagination ──────────────────────────────────────────

    pub(crate) fn refresh_search(&mut self, backend: &PickerBackend) {
        self.exact_files.clear();
        self.line_results.clear();
        self.other_files.clear();
        self.fuzzy_total_matched = 0;
        self.cumulative_grep_matched = 0;
        self.grep_next_file_offset = 0;
        self.unified_page_index = 0;
        self.unified_page_offsets = vec![(0, 0)];
        self.selected = 0;
        self.scroll_offset = 0;

        let output = backend.search(
            &self.query,
            self.search_mode,
            self.search_scope,
            self.current_file.as_deref(),
            self.force_combo_boost,
            0,
            0,
            self.page_size,
        );

        self.exact_files = output.exact_files;
        self.line_results = output.line_results;
        self.other_files = output.other_files;
        self.fuzzy_total_matched = output.fuzzy_total_matched;
        self.cumulative_grep_matched = output.grep_page_matched;
        self.grep_next_file_offset = output.grep_next_file_offset;
        self.highlight_query = output.highlight_query;

        self.rebuild_results();
        self.total_files = backend.total_files();
        self.last_search_refresh = Instant::now();

        // If the screen isn't filled, try to load more immediately.
        // Skip for Unified mode so the user sees page 0 before we replace it.
        let visible = self.results_visible_count();
        if self.search_scope != SearchScope::Unified && self.results.len() < visible.saturating_mul(2) {
            self.maybe_load_more(backend);
        }
    }

    fn rebuild_results(&mut self) {
        let mut unified = Vec::with_capacity(
            self.exact_files.len() + self.line_results.len() + self.other_files.len(),
        );
        unified.extend(self.exact_files.clone());

        if self.search_mode.group_grep && !self.line_results.is_empty() {
            let mut grouped = Vec::new();
            let mut last_path: Option<String> = None;
            for r in &self.line_results {
                if last_path.as_ref() != Some(&r.relative_path) {
                    grouped.push(UnifiedResult {
                        kind: MatchKind::FileHeader,
                        relative_path: r.relative_path.clone(),
                        absolute_path: r.absolute_path.clone(),
                        score: 0,
                        exact_match: false,
                        git_status: r.git_status.clone(),
                        ..Default::default()
                    });
                    last_path = Some(r.relative_path.clone());
                }
                grouped.push(r.clone());
            }
            unified.extend(grouped);
        } else {
            unified.extend(self.line_results.clone());
        }

        unified.extend(self.other_files.clone());
        self.results = unified;
        self.total_matched = self.fuzzy_total_matched + self.cumulative_grep_matched;
    }

    fn maybe_load_more(&mut self, backend: &PickerBackend) {
        const THRESHOLD: usize = 10;

        if self.results.is_empty() {
            return;
        }

        let remaining = self.results.len().saturating_sub(self.selected + 1);
        let threshold = if self.search_scope == SearchScope::Unified {
            0
        } else {
            THRESHOLD
        };
        if remaining > threshold {
            return;
        }

        let fuzzy_offset = self.exact_files.len() + self.other_files.len();
        let need_fuzzy = self.search_scope != SearchScope::GrepOnly
            && fuzzy_offset < self.fuzzy_total_matched;
        let need_grep = self.search_scope != SearchScope::FileOnly
            && self.grep_next_file_offset > 0;

        if !need_fuzzy && !need_grep {
            return;
        }

        if self.search_scope == SearchScope::Unified {
            let next_fuzzy_offset = fuzzy_offset;
            let next_grep_file_offset = self.grep_next_file_offset;

            let output = backend.search(
                &self.query,
                self.search_mode,
                self.search_scope,
                self.current_file.as_deref(),
                self.force_combo_boost,
                next_fuzzy_offset,
                next_grep_file_offset,
                self.page_size,
            );

            if output.results.is_empty() {
                return;
            }

            // Page replacement: discard current page and show the next one.
            self.exact_files = output.exact_files;
            self.line_results = output.line_results;
            self.other_files = output.other_files;
            self.fuzzy_total_matched = output.fuzzy_total_matched;
            self.cumulative_grep_matched = output.grep_page_matched;
            self.grep_next_file_offset = output.grep_next_file_offset;

            // Record start offset for the next page so backward nav works.
            let page_start_fuzzy = next_fuzzy_offset + self.exact_files.len() + self.other_files.len();
            let page_start_grep = self.grep_next_file_offset;
            self.unified_page_index += 1;
            if self.unified_page_offsets.len() <= self.unified_page_index {
                self.unified_page_offsets.push((page_start_fuzzy, page_start_grep));
            }

            self.selected = 0;
            self.scroll_offset = 0;
        } else {
            let output = backend.search(
                &self.query,
                self.search_mode,
                self.search_scope,
                self.current_file.as_deref(),
                self.force_combo_boost,
                fuzzy_offset,
                self.grep_next_file_offset,
                self.page_size,
            );

            if output.results.is_empty() {
                return;
            }

            if need_fuzzy {
                self.exact_files.extend(output.exact_files);
                self.other_files.extend(output.other_files);
            }

            if need_grep {
                self.line_results.extend(output.line_results);
                self.cumulative_grep_matched += output.grep_page_matched;
                self.grep_next_file_offset = output.grep_next_file_offset;
            }
        }

        self.rebuild_results();
    }

    // ── Navigation ───────────────────────────────────────────────────

    fn move_selection(&mut self, delta: isize, backend: &PickerBackend) {
        if self.results.is_empty() {
            return;
        }

        // In Unified mode, moving up past the top loads the previous page.
        if self.search_scope == SearchScope::Unified
            && delta < 0
            && self.selected == 0
            && self.unified_page_index > 0
        {
            self.load_unified_page(self.unified_page_index - 1, backend);
            self.selected = self.results.len().saturating_sub(1);
            self.ensure_visible();
            return;
        }

        let new = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected.saturating_add(delta as usize).min(self.results.len() - 1)
        };
        self.selected = new;
        self.ensure_visible();
        self.maybe_load_more(backend);
    }

    fn move_selection_page(&mut self, pages: isize) {
        let page_size = self.results_visible_count().max(1) as isize;
        self.move_selection_raw(pages * page_size);
    }

    fn move_selection_raw(&mut self, delta: isize) {
        if self.results.is_empty() {
            return;
        }
        let new = if delta < 0 {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected.saturating_add(delta as usize).min(self.results.len() - 1)
        };
        self.selected = new;
        self.ensure_visible();
    }

    fn load_unified_page(&mut self, page_index: usize, backend: &PickerBackend) {
        if page_index >= self.unified_page_offsets.len() {
            return;
        }
        let (fuzzy_offset, grep_file_offset) = self.unified_page_offsets[page_index];

        let output = backend.search(
            &self.query,
            self.search_mode,
            self.search_scope,
            self.current_file.as_deref(),
            self.force_combo_boost,
            fuzzy_offset,
            grep_file_offset,
            self.page_size,
        );

        if output.results.is_empty() && page_index > 0 {
            return;
        }

        self.exact_files = output.exact_files;
        self.line_results = output.line_results;
        self.other_files = output.other_files;
        self.fuzzy_total_matched = output.fuzzy_total_matched;
        self.cumulative_grep_matched = output.grep_page_matched;
        self.grep_next_file_offset = output.grep_next_file_offset;
        self.unified_page_index = page_index;

        self.rebuild_results();
    }

    fn results_visible_count(&self) -> usize {
        // Input bar (3) + status bar (1) + results borders (2)
        self.terminal_height.saturating_sub(6).max(1) as usize
    }

    fn ensure_visible(&mut self) {
        let visible = self.results_visible_count();
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible {
            self.scroll_offset = self.selected.saturating_sub(visible.saturating_sub(1));
        }
    }

    // ── Selection ────────────────────────────────────────────────────

    fn toggle_selection(&mut self) {
        if let Some(result) = self.results.get(self.selected) {
            let key = selection_key(result);
            if let Some(pos) = self.selected_items.iter().position(|r| selection_key(r) == key) {
                self.selected_items.remove(pos);
            } else {
                self.selected_items.push(result.clone());
            }
        }
    }

    fn select_all_visible(&mut self) {
        for result in &self.results {
            let key = selection_key(result);
            if !self.selected_items.iter().any(|r| selection_key(r) == key) {
                self.selected_items.push(result.clone());
            }
        }
    }

    fn deselect_all(&mut self) {
        self.selected_items.clear();
    }

    fn cycle_grep_mode(&mut self) {
        // plain -> regex -> fuzzy -> plain
        if self.search_mode.regex {
            self.search_mode.regex = false;
            self.search_mode.fuzzy = true;
        } else if self.search_mode.fuzzy {
            self.search_mode.fuzzy = false;
        } else {
            self.search_mode.regex = true;
        }
    }

    fn cycle_query_history(&mut self, delta: isize, backend: &PickerBackend) {
        let is_grep = self.search_scope == SearchScope::GrepOnly;
        let new_offset = if delta < 0 {
            self.query_history_offset + 1
        } else {
            self.query_history_offset.saturating_sub(1)
        };

        if new_offset == self.query_history_offset && delta > 0 {
            // Already at 0, clear query
            self.query.clear();
            self.force_combo_boost = false;
            self.refresh_search(backend);
            return;
        }

        let query = if is_grep {
            backend.get_historical_grep_query(new_offset)
        } else {
            backend.get_historical_query(new_offset)
        };

        if let Some(q) = query {
            self.query = q;
            self.force_combo_boost = true;
            self.query_history_offset = new_offset;
            self.refresh_search(backend);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picker::{MatchKind, UnifiedResult};

    fn make_file_result(path: &str) -> UnifiedResult {
        UnifiedResult {
            kind: MatchKind::File,
            relative_path: path.into(),
            absolute_path: path.into(),
            ..Default::default()
        }
    }

    fn make_line_result(path: &str, line: u64) -> UnifiedResult {
        UnifiedResult {
            kind: MatchKind::Line,
            relative_path: path.into(),
            absolute_path: path.into(),
            line_number: Some(line),
            ..Default::default()
        }
    }

    #[test]
    fn test_rebuild_results_unified() {
        let mut app = App::new();
        app.exact_files = vec![make_file_result("exact.rs")];
        app.line_results = vec![
            make_line_result("a.rs", 1),
            make_line_result("a.rs", 5),
            make_line_result("b.rs", 2),
        ];
        app.other_files = vec![make_file_result("other.rs")];
        app.search_mode.group_grep = false;
        app.fuzzy_total_matched = 2;
        app.cumulative_grep_matched = 3;

        app.rebuild_results();

        assert_eq!(app.results.len(), 5);
        assert_eq!(app.results[0].kind, MatchKind::File);
        assert_eq!(app.results[1].kind, MatchKind::Line);
        assert_eq!(app.results[4].kind, MatchKind::File);
        assert_eq!(app.total_matched, 5);
    }

    #[test]
    fn test_rebuild_results_grouped() {
        let mut app = App::new();
        app.exact_files = vec![make_file_result("exact.rs")];
        app.line_results = vec![
            make_line_result("a.rs", 1),
            make_line_result("a.rs", 5),
            make_line_result("b.rs", 2),
        ];
        app.other_files = vec![make_file_result("other.rs")];
        app.search_mode.group_grep = true;
        app.fuzzy_total_matched = 2;
        app.cumulative_grep_matched = 3;

        app.rebuild_results();

        // exact + header(a) + line + line + header(b) + line + other
        assert_eq!(app.results.len(), 7);
        assert_eq!(app.results[0].kind, MatchKind::File); // exact
        assert_eq!(app.results[1].kind, MatchKind::FileHeader); // a.rs
        assert_eq!(app.results[2].kind, MatchKind::Line); // a.rs:1
        assert_eq!(app.results[3].kind, MatchKind::Line); // a.rs:5
        assert_eq!(app.results[4].kind, MatchKind::FileHeader); // b.rs
        assert_eq!(app.results[5].kind, MatchKind::Line); // b.rs:2
        assert_eq!(app.results[6].kind, MatchKind::File); // other
    }

    #[test]
    fn test_move_selection_raw_within_page() {
        let mut app = App::new();
        app.results = vec![
            make_file_result("a.rs"),
            make_file_result("b.rs"),
            make_file_result("c.rs"),
        ];
        app.selected = 1;
        app.move_selection_raw(1);
        assert_eq!(app.selected, 2);

        app.move_selection_raw(-1);
        assert_eq!(app.selected, 1);
    }
}
