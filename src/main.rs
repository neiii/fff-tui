mod app;
mod debug_dump;
mod headless;
mod highlight;
mod icons;
mod picker;
mod theme;
mod tui;
mod ui;

use app::App;
use clap::Parser;
use picker::{PickerBackend, UnifiedResult};
use std::process;

#[derive(Parser, Debug)]
#[command(name = "fff")]
#[command(about = "Fast file finder — a blazingly fast fuzzy file picker")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Run headlessly and dump rendered frames to the given directory (for debugging).
    #[arg(long, value_name = "DIR")]
    dump_frames: Option<String>,

    /// Append line number to output for line matches (e.g., file:42).
    #[arg(long)]
    line: bool,

    /// Append line and column to output for line matches (e.g., file:42:1).
    #[arg(long)]
    column: bool,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Search for files
    Files {
        /// Directory to search
        #[arg(default_value = ".")]
        path: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let path = match &cli.command {
        Some(Commands::Files { path }) => path.clone(),
        None => ".".to_string(),
    };

    // Initialize backend
    let backend = match PickerBackend::new(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error initializing file picker: {e}");
            process::exit(1);
        }
    };

    // Headless frame-dump mode (no TTY required)
    if let Some(ref dump_dir) = cli.dump_frames {
        let out = std::path::Path::new(dump_dir);
        headless::run_headless_dump(&backend, out, 30);
        println!("Dumped frames to {}", out.display());
        process::exit(0);
    }

    // Setup terminal
    let mut terminal = match tui::setup_terminal() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error setting up terminal: {e}");
            process::exit(1);
        }
    };

    // Run app
    let mut app = App::new();
    let result = app.run(&mut terminal, &backend);

    // Restore terminal regardless of result
    let _ = tui::restore_terminal(&mut terminal);

    match result {
        Ok(Some(result)) => {
            let output = format_result(&result, cli.line, cli.column);
            println!("{output}");
            process::exit(0);
        }
        Ok(None) => {
            process::exit(130); // exit code 130 = cancelled (same as fzf)
        }
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    }
}

fn format_result(result: &UnifiedResult, line: bool, column: bool) -> String {
    let path = &result.absolute_path;
    // FileHeader behaves like File for output formatting
    match (column, line, result.line_number) {
        (true, _, Some(ln)) => {
            // Column is always 1 for now (beginning of line)
            format!("{}:{}:1", path, ln)
        }
        (false, true, Some(ln)) => {
            format!("{}:{}", path, ln)
        }
        _ => path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file_result() -> UnifiedResult {
        UnifiedResult {
            kind: crate::picker::MatchKind::File,
            relative_path: "src/main.rs".into(),
            absolute_path: "/dev/null/src/main.rs".into(),
            score: 0,
            exact_match: false,
            line_number: None,
            line_content: None,
            match_byte_offsets: None,
            is_definition: None,
        }
    }

    fn make_line_result() -> UnifiedResult {
        UnifiedResult {
            kind: crate::picker::MatchKind::Line,
            relative_path: "Cargo.toml".into(),
            absolute_path: "/dev/null/Cargo.toml".into(),
            score: 0,
            exact_match: false,
            line_number: Some(817),
            line_content: Some(r#"path = "television/main.rs""#.into()),
            match_byte_offsets: Some(vec![(0, 4)]),
            is_definition: Some(false),
        }
    }

    #[test]
    fn format_file_no_flags() {
        let r = make_file_result();
        assert_eq!(format_result(&r, false, false), "/dev/null/src/main.rs");
    }

    #[test]
    fn format_file_with_line_flag() {
        let r = make_file_result();
        assert_eq!(format_result(&r, true, false), "/dev/null/src/main.rs");
    }

    #[test]
    fn format_file_with_column_flag() {
        let r = make_file_result();
        assert_eq!(format_result(&r, false, true), "/dev/null/src/main.rs");
    }

    #[test]
    fn format_line_no_flags() {
        let r = make_line_result();
        assert_eq!(format_result(&r, false, false), "/dev/null/Cargo.toml");
    }

    #[test]
    fn format_line_with_line_flag() {
        let r = make_line_result();
        assert_eq!(format_result(&r, true, false), "/dev/null/Cargo.toml:817");
    }

    #[test]
    fn format_line_with_column_flag() {
        let r = make_line_result();
        assert_eq!(format_result(&r, false, true), "/dev/null/Cargo.toml:817:1");
    }

    #[test]
    fn format_line_with_both_flags() {
        let r = make_line_result();
        assert_eq!(format_result(&r, true, true), "/dev/null/Cargo.toml:817:1");
    }
}
