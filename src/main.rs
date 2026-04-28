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
use picker::{PickerBackend, SearchScope, UnifiedResult};
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

    /// Group grep results by file (shows :line:col gutter on the left).
    #[arg(long)]
    group: bool,

    /// Output multiple selections space-separated on a single line.
    #[arg(long)]
    space_separated: bool,

    /// Path shortening strategy for long directories.
    #[arg(long, value_name = "STRATEGY", default_value = "middle_number")]
    path_shorten: String,

    /// Deprioritize the active editor file in fuzzy search scoring.
    #[arg(long, value_name = "PATH")]
    current_file: Option<String>,

    /// Initial query to pre-fill the search box.
    #[arg(long, value_name = "QUERY")]
    query: Option<String>,

    /// Zed-style symbol breadcrumb (e.g. `mod foo > fn bar`). Extracts the
    /// leaf identifier and uses it as the initial query.
    #[arg(long, value_name = "SYMBOL")]
    symbol: Option<String>,

    /// If exactly one result matches the initial query, exit immediately
    /// without showing the TUI.
    #[arg(long)]
    exit_on_single: bool,

    /// Start in a specific search scope: file, grep, or unified.
    #[arg(long, value_name = "SCOPE")]
    scope: Option<String>,
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

    // Build App state early so we can handle --exit-on-single without touching the terminal.
    let mut app = App::new();
    app.search_mode.group_grep = cli.group;
    app.path_shorten_strategy = cli.path_shorten;
    app.current_file = cli.current_file;
    app.exit_on_single = cli.exit_on_single;

    if let Some(scope_str) = cli.scope {
        app.search_scope = match scope_str.as_str() {
            "file" => SearchScope::FileOnly,
            "grep" => SearchScope::GrepOnly,
            "unified" => SearchScope::Unified,
            _ => {
                eprintln!("Error: invalid scope '{}'. Expected: file, grep, unified", scope_str);
                process::exit(1);
            }
        };
    }

    let initial_query = cli.query.or_else(|| {
        cli.symbol.as_ref().map(|s| parse_symbol(s))
    });
    if let Some(q) = initial_query {
        app.query = q;
        app.cursor_position = app.query.len();
    }

    // When --exit-on-single is set, wait briefly for the background scan so
    // the initial search has files to match against.
    if app.exit_on_single {
        backend.wait_for_scan(std::time::Duration::from_secs(5));
    }

    // Run the initial search so --exit-on-single can decide whether we even
    // need a TTY.
    app.refresh_search(&backend);

    if app.exit_on_single && !app.query.is_empty() && app.results.len() == 1 {
        let result = &app.results[0];
        backend.track_access(&result.absolute_path);
        if !app.query.is_empty() {
            backend.track_query_completion(&app.query, &result.absolute_path);
        }
        println!("{}", format_result(result, cli.line, cli.column));
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

    let result = app.run(&mut terminal, &backend);

    // Restore terminal regardless of result
    let _ = tui::restore_terminal(&mut terminal);

    match result {
        Ok(Some(results)) => {
            // Track frecency + query history
            for result in &results {
                backend.track_access(&result.absolute_path);
                if !app.query.is_empty() {
                    backend.track_query_completion(&app.query, &result.absolute_path);
                }
            }

            if cli.space_separated && results.len() > 1 {
                let outputs: Vec<String> = results
                    .iter()
                    .map(|r| format_result(r, cli.line, cli.column))
                    .collect();
                println!("{}", outputs.join(" "));
            } else {
                for result in &results {
                    let output = format_result(result, cli.line, cli.column);
                    println!("{output}");
                }
            }
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

/// Parse a Zed-style symbol breadcrumb (e.g. `mod foo > fn bar`) and
/// extract the leaf identifier suitable for grepping.
fn parse_symbol(symbol: &str) -> String {
    let leaf = symbol
        .split('>')
        .last()
        .unwrap_or(symbol)
        .trim();

    let keywords = [
        "pub ", "async ", "unsafe ", "const ", "static ",
        "fn ", "mod ", "struct ", "enum ", "trait ", "impl ", "type ", "let ", "macro ",
    ];
    let mut s = leaf;
    loop {
        let mut changed = false;
        for kw in &keywords {
            if let Some(rest) = s.strip_prefix(kw) {
                s = rest;
                changed = true;
                break;
            }
        }
        if !changed {
            break;
        }
    }
    s.to_string()
}

fn format_result(result: &UnifiedResult, line: bool, column: bool) -> String {
    let path = &result.absolute_path;
    // FileHeader behaves like File for output formatting
    match (column, line, result.line_number) {
        (true, _, Some(ln)) => {
            let col = result.column.unwrap_or(1);
            format!("{}:{}:{}", path, ln, col)
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
            ..Default::default()
        }
    }

    fn make_line_result() -> UnifiedResult {
        UnifiedResult {
            kind: crate::picker::MatchKind::Line,
            relative_path: "Cargo.toml".into(),
            absolute_path: "/dev/null/Cargo.toml".into(),
            line_number: Some(817),
            column: Some(1),
            line_content: Some(r#"path = "television/main.rs""#.into()),
            match_byte_offsets: Some(vec![(0, 4)]),
            is_definition: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn test_parse_symbol_simple() {
        assert_eq!(parse_symbol("fn test_foo"), "test_foo");
    }

    #[test]
    fn test_parse_symbol_breadcrumb() {
        assert_eq!(parse_symbol("mod tests > fn test_task_contexts"), "test_task_contexts");
    }

    #[test]
    fn test_parse_symbol_with_pub_async() {
        assert_eq!(parse_symbol("pub async fn my_func"), "my_func");
    }

    #[test]
    fn test_parse_symbol_no_prefix() {
        assert_eq!(parse_symbol("MyClass > my_method"), "my_method");
    }

    #[test]
    fn test_parse_symbol_struct() {
        assert_eq!(parse_symbol("mod foo > struct Bar"), "Bar");
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
