use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub match_fg: Color,
    pub match_bg: Color,
    pub selected_bg: Color,
    pub prompt_fg: Color,
    pub status_fg: Color,
    pub status_bg: Color,
    pub badge_file_fg: Color,
    pub badge_file_bg: Color,
    pub badge_line_fg: Color,
    pub badge_line_bg: Color,
    pub preview_gutter_fg: Color,
    pub preview_highlight_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::gruvbox_dark()
    }
}

impl Theme {
    pub fn gruvbox_dark() -> Self {
        Self {
            fg: Color::Rgb(0xeb, 0xdb, 0xb2),         // #ebdbb2
            bg: Color::Rgb(0x28, 0x28, 0x28),         // #282828
            match_fg: Color::Rgb(0xfb, 0x49, 0x34),   // #fb4934 (red)
            match_bg: Color::Reset,
            selected_bg: Color::Rgb(0x50, 0x49, 0x45), // #504945
            prompt_fg: Color::Rgb(0xb8, 0xbb, 0x26), // #b8bb26 (green)
            status_fg: Color::Rgb(0xa8, 0x99, 0x84), // #a89984
            status_bg: Color::Rgb(0x3c, 0x38, 0x36), // #3c3836
            badge_file_fg: Color::Rgb(0x28, 0x28, 0x28), // dark text
            badge_file_bg: Color::Rgb(0x98, 0xc3, 0x79), // green #98c379
            badge_line_fg: Color::Rgb(0x28, 0x28, 0x28), // dark text
            badge_line_bg: Color::Rgb(0x83, 0xa5, 0x98), // cyan #83a598
            preview_gutter_fg: Color::Rgb(0x92, 0x83, 0x74), // dim #928374
            preview_highlight_bg: Color::Rgb(0x50, 0x49, 0x45), // #504945
        }
    }

    pub fn style_fg(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn style_match(&self) -> Style {
        Style::default()
            .fg(self.match_fg)
            .bg(self.match_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_selected(&self) -> Style {
        Style::default().bg(self.selected_bg)
    }

    pub fn style_prompt(&self) -> Style {
        Style::default().fg(self.prompt_fg).bg(self.bg)
    }

    pub fn style_status(&self) -> Style {
        Style::default().fg(self.status_fg).bg(self.status_bg)
    }

    pub fn style_badge_file(&self) -> Style {
        Style::default()
            .fg(self.badge_file_fg)
            .bg(self.badge_file_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_badge_line(&self) -> Style {
        Style::default()
            .fg(self.badge_line_fg)
            .bg(self.badge_line_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_preview_gutter(&self) -> Style {
        Style::default().fg(self.preview_gutter_fg).bg(self.bg)
    }

    pub fn style_preview_highlight(&self) -> Style {
        Style::default().bg(self.preview_highlight_bg)
    }
}
