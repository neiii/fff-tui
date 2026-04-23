mod app;
mod debug_dump;
mod headless;
mod highlight;
mod picker;
mod theme;
mod tui;
mod ui;

use app::App;
use clap::Parser;
use picker::PickerBackend;
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
        Ok(Some(path)) => {
            println!("{path}");
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
