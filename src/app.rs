use crate::picker::{selection_key, PickerBackend, SearchMode, SearchScope, UnifiedResult};
use crate::theme::Theme;
use crate::ui::{draw, UiState};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::collections::HashSet;
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
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<crate::tui::Backend>,
        backend: &PickerBackend,
    ) -> anyhow::Result<Option<Vec<UnifiedResult>>> {
        let tick_rate = Duration::from_millis(50);
        let scan_timeout = Duration::from_secs(5);
        let scan_start = Instant::now();
        let mut dump_frame = 0usize;

        // Phase 1: wait for the initial filesystem scan (with timeout).
        // Draw a spinner so the user sees activity instead of "0 files".
        while backend.is_scanning() && scan_start.elapsed() < scan_timeout {
            if self.last_spinner_tick.elapsed() > Duration::from_millis(80) {
                self.spinner_frame += 1;
                self.last_spinner_tick = Instant::now();
            }

            self.total_files = backend.total_files();

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
                is_scanning: true,
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
                    Event::Key(key) => {
                        if key.code == KeyCode::Esc
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            self.should_quit = true;
                            return Ok(None);
                        }
                    }
                    Event::Resize(width, height) => {
                        self.terminal_width = width;
                        self.terminal_height = height;
                    }
                    _ => {}
                }
            }
        }

        // Initial search now that the scan has finished (or timed out).
        self.refresh_search(backend);

        while !self.should_quit && !self.should_select {
            if self.last_spinner_tick.elapsed() > Duration::from_millis(80) {
                self.spinner_frame += 1;
                self.last_spinner_tick = Instant::now();
            }

            let is_scanning = backend.is_scanning();

            // Refresh search periodically while scanning so files appear incrementally
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
                        'n' => self.move_selection(1),
                        'p' => self.move_selection(-1),
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
                    self.move_selection(-1);
                }
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.cycle_query_history(1, backend);
                } else {
                    self.move_selection(1);
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

    pub(crate) fn refresh_search(&mut self, backend: &PickerBackend) {
        let limit = 500; // fetch enough for smooth scrolling
        let output = backend.search(
            &self.query,
            self.search_mode,
            self.search_scope,
            self.current_file.as_deref(),
            self.force_combo_boost,
            limit,
        );
        self.results = output.results;
        self.total_matched = output.total_matched;
        self.total_files = backend.total_files();
        self.highlight_query = output.highlight_query;
        self.selected = 0;
        self.scroll_offset = 0;
        self.last_search_refresh = Instant::now();
    }

    fn move_selection(&mut self, delta: isize) {
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

    fn move_selection_page(&mut self, pages: isize) {
        let page_size = self.results_visible_count().max(1) as isize;
        self.move_selection(pages * page_size);
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
