use crate::picker::{FileResult, PickerBackend};
use crate::theme::Theme;
use crate::ui::{draw, UiState};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::time::{Duration, Instant};

pub struct App {
    pub query: String,
    pub highlight_query: String,
    pub results: Vec<FileResult>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub total_files: usize,
    pub total_matched: usize,
    pub should_quit: bool,
    pub should_select: bool,
    pub spinner_frame: usize,
    pub last_spinner_tick: Instant,
    pub terminal_height: u16,
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
            spinner_frame: 0,
            last_spinner_tick: Instant::now(),
            terminal_height: 24,
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<crate::tui::Backend>,
        backend: &PickerBackend,
    ) -> anyhow::Result<Option<String>> {
        // Initial search (empty query shows all files sorted by frecency)
        self.refresh_search(backend);
        self.total_files = backend.total_files();

        let tick_rate = Duration::from_millis(50);

        while !self.should_quit && !self.should_select {
            // Update spinner
            if self.last_spinner_tick.elapsed() > Duration::from_millis(80) {
                self.spinner_frame += 1;
                self.last_spinner_tick = Instant::now();
            }

            // Check if scan finished
            if backend.is_scanning() {
                self.total_files = backend.total_files();
            }

            // Update terminal size for scroll calculations
            if let Ok(size) = terminal.size() {
                self.terminal_height = size.height;
            }

            // Draw
            let ui_state = UiState {
                query: self.query.clone(),
                highlight_query: self.highlight_query.clone(),
                results: self.results.clone(),
                selected: self.selected,
                scroll_offset: self.scroll_offset,
                total_files: self.total_files,
                total_matched: self.total_matched,
                is_scanning: backend.is_scanning(),
                spinner_frame: self.spinner_frame,
            };
            terminal.draw(|f| draw(f, &ui_state, &Theme::default()))?;

            // Handle events
            if event::poll(tick_rate)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key, backend),
                    Event::Resize(_, height) => {
                        self.terminal_height = height;
                        self.ensure_visible();
                    }
                    _ => {}
                }
            }
        }

        if self.should_select {
            Ok(self.results.get(self.selected).map(|r| r.absolute_path.clone()))
        } else {
            Ok(None)
        }
    }

    fn handle_key(&mut self, key: KeyEvent, backend: &PickerBackend) {
        match key.code {
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match c {
                        'c' | 'd' => self.should_quit = true,
                        'n' => self.move_selection(1),
                        'p' => self.move_selection(-1),
                        'u' => self.move_selection_page(-1),
                        'l' => {
                            self.query.clear();
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
            KeyCode::Enter => {
                if !self.results.is_empty() {
                    self.should_select = true;
                }
            }
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::PageUp => self.move_selection_page(-1),
            KeyCode::PageDown => self.move_selection_page(1),
            KeyCode::Home => {
                self.selected = 0;
                self.scroll_offset = 0;
            }
            KeyCode::End => {
                if !self.results.is_empty() {
                    self.selected = self.results.len() - 1;
                    self.ensure_visible();
                }
            }
            _ => {}
        }
    }

    fn refresh_search(&mut self, backend: &PickerBackend) {
        let limit = 500; // fetch enough for smooth scrolling
        let output = backend.search(&self.query, limit);
        self.results = output.results;
        self.total_matched = output.total_matched;
        self.highlight_query = output.highlight_query;
        self.selected = 0;
        self.scroll_offset = 0;
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
        // Status bar (1) + Input (3)
        self.terminal_height.saturating_sub(4) as usize
    }

    fn ensure_visible(&mut self) {
        let visible = self.results_visible_count();
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible {
            self.scroll_offset = self.selected.saturating_sub(visible.saturating_sub(1));
        }
    }
}
