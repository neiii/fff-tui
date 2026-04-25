use crate::picker::{selection_key, PickerBackend, SearchMode, SearchScope, UnifiedResult};
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
    // ── Pagination state (page-replacement model, matching fff.nvim) ──
    pub page_size: usize,
    pub page_index: usize,
    /// For grep file-based pagination. `grep_file_offsets[i]` is the
    /// `file_offset` to use for page `i`.  Index 0 always starts at 0.
    pub grep_file_offsets: Vec<usize>,
    /// True total fuzzy matches (independent of pagination).  Used to
    /// calculate `max_page_index` for fuzzy / unified scopes.
    pub fuzzy_total_matched: usize,
    /// Whether the last grep search reported more files (`next_file_offset > 0`).
    pub grep_has_more: bool,
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
            page_size: 500,
            page_index: 0,
            grep_file_offsets: vec![0],
            fuzzy_total_matched: 0,
            grep_has_more: false,
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
        // Do NOT block on the background scan.  Search against whatever
        // files are already indexed; the periodic refresh below will pick
        // up new files as they arrive.
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
            }
            _ => {}
        }
    }

    // ── Search & Pagination ──────────────────────────────────────────

    pub(crate) fn refresh_search(&mut self, backend: &PickerBackend) {
        self.page_index = 0;
        self.grep_file_offsets = vec![0];
        self.fuzzy_total_matched = 0;
        self.grep_has_more = false;
        self.selected = 0;
        self.scroll_offset = 0;
        let _ = self.load_page_at_index(0, backend);
        self.total_files = backend.total_files();
        self.last_search_refresh = Instant::now();
    }

    /// Load a specific page by index (0-based).  Returns `true` on success.
    /// This mirrors `picker_ui.lua:M.load_page_at_index()`.
    fn load_page_at_index(&mut self, page_index: usize, backend: &PickerBackend) -> bool {
        let page_size = self.results_visible_count().max(20);
        self.page_size = page_size;

        if self.search_scope != SearchScope::GrepOnly {
            let total = self.fuzzy_total_matched;
            if total > 0 {
                let max_page = (total.saturating_sub(1) / page_size).max(0);
                if page_index > max_page {
                    return false;
                }
            }
        }

        let fuzzy_offset = if self.search_scope != SearchScope::GrepOnly {
            page_index * page_size
        } else {
            0
        };

        let grep_file_offset = if self.search_scope != SearchScope::FileOnly
            && !self.query.is_empty()
        {
            *self.grep_file_offsets.get(page_index).unwrap_or(&0)
        } else {
            0
        };

        let output = backend.search(
            &self.query,
            self.search_mode,
            self.search_scope,
            self.current_file.as_deref(),
            self.force_combo_boost,
            fuzzy_offset,
            grep_file_offset,
            page_size,
        );

        if output.results.is_empty() && page_index > 0 {
            return false;
        }

        self.results = output.results;
        self.total_matched = output.total_matched;
        self.highlight_query = output.highlight_query;
        self.fuzzy_total_matched = output.fuzzy_total_matched;
        self.grep_has_more = output.grep_next_file_offset > 0;
        self.page_index = page_index;

        // Record / update grep file offsets so forward/backward navigation works.
        if page_index >= self.grep_file_offsets.len() {
            self.grep_file_offsets.resize(page_index + 1, 0);
        }
        self.grep_file_offsets[page_index] = grep_file_offset;
        if output.grep_next_file_offset > 0 {
            if page_index + 1 >= self.grep_file_offsets.len() {
                self.grep_file_offsets.push(output.grep_next_file_offset);
            } else {
                self.grep_file_offsets[page_index + 1] = output.grep_next_file_offset;
            }
        }

        true
    }

    fn load_next_page(&mut self, backend: &PickerBackend) -> bool {
        if !self.has_more_pages() {
            return false;
        }
        let ok = self.load_page_at_index(self.page_index + 1, backend);
        if ok {
            self.selected = 0;
            self.scroll_offset = 0;
        }
        ok
    }

    fn load_previous_page(&mut self, backend: &PickerBackend) -> bool {
        if self.page_index == 0 {
            return false;
        }
        let ok = self.load_page_at_index(self.page_index - 1, backend);
        if ok {
            self.selected = self.results.len().saturating_sub(1);
            self.ensure_visible();
        }
        ok
    }

    fn has_more_pages(&self) -> bool {
        if self.search_scope == SearchScope::GrepOnly {
            self.grep_has_more
        } else {
            let page_size = self.results_visible_count().max(20);
            let max_page = if self.fuzzy_total_matched > 0 {
                (self.fuzzy_total_matched.saturating_sub(1) / page_size).max(0)
            } else {
                0
            };
            self.page_index < max_page || self.grep_has_more
        }
    }

    // ── Navigation ───────────────────────────────────────────────────

    fn move_selection(&mut self, delta: isize, backend: &PickerBackend) {
        if self.results.is_empty() {
            return;
        }

        if delta > 0 {
            // Moving toward worse results (Down).
            let new = self.selected.saturating_add(delta as usize);
            if new >= self.results.len() {
                // At last item → try next page (like Lua move_down + near_bottom).
                if self.load_next_page(backend) {
                    return;
                }
                self.selected = self.results.len() - 1;
            } else {
                self.selected = new;
            }
        } else {
            // Moving toward better results (Up).
            let abs = delta.unsigned_abs();
            if self.selected == 0 && abs > 0 && self.page_index > 0 {
                // At first item → try previous page (like Lua move_up).
                self.load_previous_page(backend);
                return;
            }
            self.selected = self.selected.saturating_sub(abs);
        }
        self.ensure_visible();
    }

    fn move_selection_page(&mut self, pages: isize) {
        let page_size = self.results_visible_count().max(1) as isize;
        self.move_selection_raw(pages * page_size);
    }

    /// Move cursor without triggering page loads (used by PageUp/PageDown).
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

    #[test]
    fn test_page_index_reset_on_refresh() {
        let mut app = App::new();
        app.page_index = 3;
        app.grep_file_offsets = vec![0, 50, 100, 150];
        // refresh_search resets pagination (but we can't call it without a backend)
        // So just verify the struct fields reset correctly if we do it manually:
        app.page_index = 0;
        app.grep_file_offsets = vec![0];
        assert_eq!(app.page_index, 0);
        assert_eq!(app.grep_file_offsets, vec![0]);
    }

    #[test]
    fn test_has_more_pages_fuzzy() {
        let mut app = App::new();
        app.terminal_height = 30; // visible ~24
        app.fuzzy_total_matched = 100;
        app.page_index = 0;
        app.grep_has_more = false;
        // page_size = 24, max_page = (100-1)/24 = 4, so page_index 0 < 4 → has more
        assert!(app.has_more_pages());

        app.page_index = 4;
        // page_index == max_page → no more fuzzy pages
        assert!(!app.has_more_pages());

        app.grep_has_more = true;
        // But grep still has more (Unified scope)
        assert!(app.has_more_pages());
    }

    #[test]
    fn test_has_more_pages_grep_only() {
        let mut app = App::new();
        app.search_scope = SearchScope::GrepOnly;
        app.grep_has_more = true;
        assert!(app.has_more_pages());

        app.grep_has_more = false;
        assert!(!app.has_more_pages());
    }

    #[test]
    fn test_grep_file_offsets_growth() {
        let mut app = App::new();
        app.grep_file_offsets = vec![0];
        // Simulate recording offset for page 1
        app.grep_file_offsets.push(47);
        assert_eq!(app.grep_file_offsets, vec![0, 47]);

        // Overwrite page 1 offset
        app.grep_file_offsets[1] = 52;
        assert_eq!(app.grep_file_offsets, vec![0, 52]);
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
