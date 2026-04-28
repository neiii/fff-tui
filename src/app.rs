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
    /// Byte offset of the cursor inside `query`.
    pub cursor_position: usize,
    /// Receiver for background exhaustive search results.
    pub background_rx: Option<std::sync::mpsc::Receiver<(String, crate::picker::SearchOutput)>>,
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
            cursor_position: 0,
            background_rx: None,
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<crate::tui::Backend>,
        backend: &PickerBackend,
    ) -> anyhow::Result<Option<Vec<UnifiedResult>>> {
        let tick_rate = Duration::from_millis(50);
        let mut dump_frame = 0usize;

        // Hide the blinking hardware cursor — we render our own input state.
        terminal.hide_cursor()?;

        // ── Open immediately (like Lua extension) ──
        self.refresh_search(backend);

        while !self.should_quit && !self.should_select {
            // Apply background exhaustive search results when ready.
            if let Some(ref rx) = self.background_rx {
                if let Ok((query, output)) = rx.try_recv() {
                    if self.query == query {
                        self.results = output.results;
                        self.total_matched = output.fuzzy_total_matched + output.grep_page_matched;
                        self.highlight_query = output.highlight_query;
                        if !self.results.is_empty() && self.selected >= self.results.len() {
                            self.selected = self.results.len() - 1;
                        }
                        self.ensure_visible();
                    }
                    self.background_rx = None;
                }
            }

            if self.last_spinner_tick.elapsed() > Duration::from_millis(80) {
                self.spinner_frame += 1;
                self.last_spinner_tick = Instant::now();
            }

            let is_scanning = backend.is_scanning();

            // Refresh search periodically while scanning so files appear
            // incrementally (matching Lua's monitor_scan_progress).
            // Also refresh when the query is empty but we still have no
            // results, which happens if the scan finished between the
            // initial refresh and the first timer tick.
            let needs_refresh = is_scanning
                || (self.query.is_empty() && self.results.is_empty());
            if needs_refresh && self.last_search_refresh.elapsed() > Duration::from_millis(200) {
                self.refresh_search(backend);
            }
            if is_scanning {
                self.total_files = backend.total_files();
            }

            if let Ok(size) = terminal.size() {
                self.terminal_height = size.height;
                self.terminal_width = size.width;
            }

            let ui_state = UiState {
                query: self.query.clone(),
                highlight_query: self.highlight_query.clone(),
                cursor_position: self.cursor_position,
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
                        'a' => self.cursor_position = 0,
                        'e' => self.cursor_position = self.query.len(),
                        'd' => {
                            if self.cursor_position < self.query.len() {
                                let ch = self.query[self.cursor_position..].chars().next().unwrap();
                                let len = ch.len_utf8();
                                self.query.replace_range(self.cursor_position..self.cursor_position + len, "");
                                self.refresh_search(backend);
                            }
                        }
                        'k' => {
                            if self.cursor_position < self.query.len() {
                                self.query.truncate(self.cursor_position);
                                self.refresh_search(backend);
                            }
                        }
                        'w' => {
                            self.delete_word_backward();
                            self.refresh_search(backend);
                        }
                        'u' | 'l' => {
                            self.query.clear();
                            self.cursor_position = 0;
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
                            self.delete_word_backward();
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
                        'b' => self.move_cursor_word_backward(),
                        _ => {}
                    }
                } else {
                    self.query.insert(self.cursor_position, c);
                    self.cursor_position += c.len_utf8();
                    self.refresh_search(backend);
                }
            }
            KeyCode::Backspace => {
                if key.modifiers.contains(KeyModifiers::SUPER) {
                    self.query.clear();
                    self.cursor_position = 0;
                    self.refresh_search(backend);
                } else if key.modifiers.contains(KeyModifiers::ALT)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.delete_word_backward();
                    self.refresh_search(backend);
                } else if self.cursor_position > 0 {
                    let ch = self.query[..self.cursor_position].chars().last().unwrap();
                    let len = ch.len_utf8();
                    self.query.replace_range(self.cursor_position - len..self.cursor_position, "");
                    self.cursor_position -= len;
                    self.refresh_search(backend);
                }
            }
            KeyCode::Delete => {
                if key.modifiers.contains(KeyModifiers::SUPER) {
                    self.query.clear();
                    self.cursor_position = 0;
                    self.refresh_search(backend);
                } else if key.modifiers.contains(KeyModifiers::ALT)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    self.delete_word_backward();
                    self.refresh_search(backend);
                } else if self.cursor_position < self.query.len() {
                    let ch = self.query[self.cursor_position..].chars().next().unwrap();
                    let len = ch.len_utf8();
                    self.query.replace_range(self.cursor_position..self.cursor_position + len, "");
                    self.refresh_search(backend);
                }
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
            KeyCode::Left => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.move_cursor_word_backward();
                } else if key.modifiers.contains(KeyModifiers::SUPER) {
                    self.cursor_position = 0;
                } else {
                    self.move_cursor_left();
                }
            }
            KeyCode::Right => {
                if key.modifiers.contains(KeyModifiers::ALT) {
                    self.move_cursor_word_forward();
                } else if key.modifiers.contains(KeyModifiers::SUPER) {
                    self.cursor_position = self.query.len();
                } else {
                    self.move_cursor_right();
                }
            }
            KeyCode::Home => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.selected = 0;
                    self.scroll_offset = 0;
                } else {
                    self.cursor_position = 0;
                }
            }
            KeyCode::End => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    if !self.results.is_empty() {
                        self.selected = self.results.len() - 1;
                        self.ensure_visible();
                    }
                } else {
                    self.cursor_position = self.query.len();
                }
            }
            _ => {}
        }
    }

    // ── Search ───────────────────────────────────────────────────────

    pub(crate) fn refresh_search(&mut self, backend: &PickerBackend) {
        self.selected = 0;
        self.scroll_offset = 0;

        // 1. Quick initial search — limited results so the UI feels instant.
        const INITIAL_LIMIT: usize = 200;
        const INITIAL_TIME_BUDGET_MS: u64 = 150;

        let output = backend.search(
            &self.query,
            self.search_mode,
            self.search_scope,
            self.current_file.as_deref(),
            self.force_combo_boost,
            INITIAL_LIMIT,
            INITIAL_TIME_BUDGET_MS,
        );

        self.results = output.results;
        self.total_matched = output.fuzzy_total_matched + output.grep_page_matched;
        self.highlight_query = output.highlight_query;
        self.total_files = backend.total_files();
        self.last_search_refresh = Instant::now();

        // 2. Spawn background thread for the full exhaustive search.
        let (tx, rx) = std::sync::mpsc::channel();
        let backend_clone = backend.clone();
        let query = self.query.clone();
        let mode = self.search_mode;
        let scope = self.search_scope;
        let current_file = self.current_file.clone();
        let force_combo = self.force_combo_boost;

        std::thread::spawn(move || {
            let full = backend_clone.search(
                &query, mode, scope, current_file.as_deref(), force_combo,
                usize::MAX, 0,
            );
            let _ = tx.send((query, full));
        });

        self.background_rx = Some(rx);
    }

    // ── Navigation ───────────────────────────────────────────────────

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

    fn move_cursor_left(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let ch = self.query[..self.cursor_position].chars().last().unwrap();
        self.cursor_position -= ch.len_utf8();
    }

    fn move_cursor_right(&mut self) {
        if self.cursor_position >= self.query.len() {
            return;
        }
        let ch = self.query[self.cursor_position..].chars().next().unwrap();
        self.cursor_position += ch.len_utf8();
    }

    fn move_cursor_word_backward(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        // Skip whitespace before cursor
        while self.cursor_position > 0 {
            let c = self.query[..self.cursor_position].chars().last().unwrap();
            if c.is_whitespace() {
                self.cursor_position -= c.len_utf8();
            } else {
                break;
            }
        }
        // Skip word characters
        while self.cursor_position > 0 {
            let c = self.query[..self.cursor_position].chars().last().unwrap();
            if !c.is_whitespace() {
                self.cursor_position -= c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn move_cursor_word_forward(&mut self) {
        if self.cursor_position >= self.query.len() {
            return;
        }
        // Skip word characters after cursor
        while self.cursor_position < self.query.len() {
            let c = self.query[self.cursor_position..].chars().next().unwrap();
            if !c.is_whitespace() {
                self.cursor_position += c.len_utf8();
            } else {
                break;
            }
        }
        // Skip whitespace after cursor
        while self.cursor_position < self.query.len() {
            let c = self.query[self.cursor_position..].chars().next().unwrap();
            if c.is_whitespace() {
                self.cursor_position += c.len_utf8();
            } else {
                break;
            }
        }
    }

    fn delete_word_backward(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let mut end = self.cursor_position;
        // Skip trailing whitespace before cursor
        while end > 0 {
            if let Some(c) = self.query[..end].chars().last() {
                if c.is_whitespace() {
                    end -= c.len_utf8();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        // Skip word characters
        while end > 0 {
            if let Some(c) = self.query[..end].chars().last() {
                if !c.is_whitespace() {
                    end -= c.len_utf8();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        self.query.replace_range(end..self.cursor_position, "");
        self.cursor_position = end;
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
            self.cursor_position = 0;
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
            self.cursor_position = q.len();
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

    #[test]
    fn test_delete_word_backward_basic() {
        let mut app = App::new();
        app.query = "hello world".into();
        app.cursor_position = app.query.len();
        app.delete_word_backward();
        assert_eq!(app.query, "hello ");

        app.delete_word_backward();
        assert_eq!(app.query, "");
    }

    #[test]
    fn test_delete_word_backward_trailing_space() {
        let mut app = App::new();
        app.query = "foo bar  ".into();
        app.cursor_position = app.query.len();
        app.delete_word_backward();
        assert_eq!(app.query, "foo ");
    }

    #[test]
    fn test_delete_word_backward_empty() {
        let mut app = App::new();
        app.query = "".into();
        app.cursor_position = 0;
        app.delete_word_backward();
        assert_eq!(app.query, "");
    }

    #[test]
    fn test_move_cursor_left_right() {
        let mut app = App::new();
        app.query = "abc".into();
        app.cursor_position = 3;
        app.move_cursor_left();
        assert_eq!(app.cursor_position, 2);
        app.move_cursor_right();
        assert_eq!(app.cursor_position, 3);
    }

    #[test]
    fn test_move_cursor_word_backward() {
        let mut app = App::new();
        app.query = "hello world".into();
        app.cursor_position = app.query.len();
        app.move_cursor_word_backward();
        assert_eq!(app.cursor_position, "hello ".len());
        app.move_cursor_word_backward();
        assert_eq!(app.cursor_position, 0);
    }

    #[test]
    fn test_move_cursor_word_forward() {
        let mut app = App::new();
        app.query = "hello world".into();
        app.cursor_position = 0;
        app.move_cursor_word_forward();
        // Skips "hello" and the trailing space
        assert_eq!(app.cursor_position, "hello ".len());
        app.move_cursor_word_forward();
        assert_eq!(app.cursor_position, app.query.len());
    }

    #[test]
    fn test_insert_at_cursor() {
        let mut app = App::new();
        app.query = "ac".into();
        app.cursor_position = 1;
        app.query.insert(app.cursor_position, 'b');
        app.cursor_position += 'b'.len_utf8();
        assert_eq!(app.query, "abc");
        assert_eq!(app.cursor_position, 2);
    }

    #[test]
    fn test_backspace_at_cursor() {
        let mut app = App::new();
        app.query = "abc".into();
        app.cursor_position = 2;
        let ch = app.query[..app.cursor_position].chars().last().unwrap();
        let len = ch.len_utf8();
        app.query.replace_range(app.cursor_position - len..app.cursor_position, "");
        app.cursor_position -= len;
        assert_eq!(app.query, "ac");
        assert_eq!(app.cursor_position, 1);
    }
}
