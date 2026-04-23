use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, stdout, stderr, IsTerminal, Write};

pub type Backend = CrosstermBackend<Box<dyn Write>>;

pub fn setup_terminal() -> io::Result<Terminal<Backend>> {
    // Try stdout first (normal interactive use)
    if stdout().is_terminal() {
        let mut out: Box<dyn Write> = Box::new(stdout());
        enable_raw_mode()?;
        out.execute(EnterAlternateScreen)?;
        return Ok(Terminal::new(CrosstermBackend::new(out))?);
    }

    // Fallback to stderr (e.g. when stdout is captured by shell command substitution)
    if stderr().is_terminal() {
        let mut out: Box<dyn Write> = Box::new(stderr());
        enable_raw_mode()?;
        out.execute(EnterAlternateScreen)?;
        return Ok(Terminal::new(CrosstermBackend::new(out))?);
    }

    Err(io::Error::new(
        io::ErrorKind::Other,
        "No TTY available. fff requires an interactive terminal.",
    ))
}

pub fn restore_terminal(terminal: &mut Terminal<Backend>) -> io::Result<()> {
    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
