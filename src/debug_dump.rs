use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use std::path::Path;

/// Serialize a ratatui Buffer to a plain-text string.
/// Multi-width symbols are printed as-is; empty cells become spaces.
pub fn buffer_to_string(buffer: &Buffer) -> String {
    let area = buffer.area();
    let mut lines: Vec<String> = Vec::with_capacity(area.height as usize);

    for y in area.y..area.y + area.height {
        let mut line = String::with_capacity(area.width as usize);
        for x in area.x..area.x + area.width {
            let cell = buffer.cell(Position { x, y }).unwrap_or_else(|| {
                // Should never happen for in-bounds coords, but be defensive
                panic!("missing cell at ({x}, {y})");
            });
            let symbol = cell.symbol();
            if symbol.is_empty() {
                line.push(' ');
            } else {
                line.push_str(symbol);
            }
        }
        lines.push(line);
    }

    lines.join("\n") + "\n"
}

/// Write a frame buffer to `$FFF_DUMP/frame-{count:04}.txt`.
pub fn dump_buffer(buffer: &Buffer, count: usize) {
    if let Ok(dir) = std::env::var("FFF_DUMP") {
        dump_buffer_to_dir(buffer, count, Path::new(&dir));
    }
}

/// Write a frame buffer to `{dir}/frame-{count:04}.txt`.
pub fn dump_buffer_to_dir(buffer: &Buffer, count: usize, dir: &Path) {
    if std::fs::create_dir_all(dir).is_ok() {
        let file = dir.join(format!("frame-{count:04}.txt"));
        let text = buffer_to_string(buffer);
        let _ = std::fs::write(&file, text);
    }
}
