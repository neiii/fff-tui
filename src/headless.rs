use crate::app::App;
use crate::debug_dump::dump_buffer_to_dir;
use crate::picker::PickerBackend;
use crate::theme::Theme;
use crate::ui::{draw, UiState};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::path::Path;
use std::time::{Duration, Instant};

/// Run the app headlessly for a short duration and dump rendered frames to disk.
/// Used by agents to verify TUI output without an interactive terminal.
pub fn run_headless_dump(backend: &PickerBackend, out_dir: &Path, max_frames: usize) {
    std::fs::create_dir_all(out_dir).ok();

    let backend_size = TestBackend::new(120, 40);
    let mut terminal = Terminal::new(backend_size).unwrap();

    let mut app = App::new();
    let scan_timeout = Duration::from_secs(5);
    let scan_start = Instant::now();
    let tick_rate = Duration::from_millis(50);
    let mut frame = 0usize;

    // Phase 1: simulate scanning wait
    while backend.is_scanning() && scan_start.elapsed() < scan_timeout && frame < max_frames {
        if app.last_spinner_tick.elapsed() > Duration::from_millis(80) {
            app.spinner_frame += 1;
            app.last_spinner_tick = Instant::now();
        }
        app.total_files = backend.total_files();

        let ui_state = UiState {
            query: app.query.clone(),
            highlight_query: app.highlight_query.clone(),
            results: app.results.clone(),
            selected: app.selected,
            scroll_offset: app.scroll_offset,
            total_files: app.total_files,
            total_matched: app.total_matched,
            is_scanning: true,
            spinner_frame: app.spinner_frame,
        };
        terminal
            .draw(|f| {
                draw(f, &ui_state, &Theme::default());
                dump_buffer_to_dir(f.buffer_mut(), frame, out_dir);
                frame += 1;
            })
            .unwrap();

        std::thread::sleep(tick_rate);
    }

    // Initial search
    app.refresh_search(backend);

    // Simulate a few frames of idle state
    for _ in 0..max_frames.saturating_sub(frame).min(10) {
        if app.last_spinner_tick.elapsed() > Duration::from_millis(80) {
            app.spinner_frame += 1;
            app.last_spinner_tick = Instant::now();
        }
        let is_scanning = backend.is_scanning();
        if is_scanning && app.last_search_refresh.elapsed() > Duration::from_millis(200) {
            app.refresh_search(backend);
        }
        app.terminal_height = 40;

        let ui_state = UiState {
            query: app.query.clone(),
            highlight_query: app.highlight_query.clone(),
            results: app.results.clone(),
            selected: app.selected,
            scroll_offset: app.scroll_offset,
            total_files: app.total_files,
            total_matched: app.total_matched,
            is_scanning,
            spinner_frame: app.spinner_frame,
        };
        terminal
            .draw(|f| {
                draw(f, &ui_state, &Theme::default());
                dump_buffer_to_dir(f.buffer_mut(), frame, out_dir);
                frame += 1;
            })
            .unwrap();

        std::thread::sleep(tick_rate);
    }

    // Also simulate typing a query
    app.query.push_str("main");
    app.refresh_search(backend);
    for _ in 0..5 {
        let ui_state = UiState {
            query: app.query.clone(),
            highlight_query: app.highlight_query.clone(),
            results: app.results.clone(),
            selected: app.selected,
            scroll_offset: app.scroll_offset,
            total_files: app.total_files,
            total_matched: app.total_matched,
            is_scanning: backend.is_scanning(),
            spinner_frame: app.spinner_frame,
        };
        terminal
            .draw(|f| {
                draw(f, &ui_state, &Theme::default());
                dump_buffer_to_dir(f.buffer_mut(), frame, out_dir);
                frame += 1;
            })
            .unwrap();
        std::thread::sleep(tick_rate);
    }
}
