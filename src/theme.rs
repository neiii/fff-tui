use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    pub match_fg: Color,
    pub match_bg: Color,
    pub selected_bg: Color,
    pub border_fg: Color,
    pub prompt_fg: Color,
    pub status_fg: Color,
    pub status_bg: Color,
    pub spinner_fg: Color,
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
            highlight_fg: Color::Rgb(0xfb, 0xf1, 0xc7), // #fbf1c7
            highlight_bg: Color::Rgb(0x45, 0x45, 0x45), // #454545
            match_fg: Color::Rgb(0xfb, 0x49, 0x34),   // #fb4934 (red)
            match_bg: Color::Reset,
            selected_bg: Color::Rgb(0x50, 0x49, 0x45), // #504945
            border_fg: Color::Rgb(0x66, 0x5c, 0x54), // #665c54
            prompt_fg: Color::Rgb(0xb8, 0xbb, 0x26), // #b8bb26 (green)
            status_fg: Color::Rgb(0xa8, 0x99, 0x84), // #a89984
            status_bg: Color::Rgb(0x3c, 0x38, 0x36), // #3c3836
            spinner_fg: Color::Rgb(0xfe, 0x80, 0x19), // #fe8019 (orange)
        }
    }

    pub fn catppuccin_mocha() -> Self {
        Self {
            fg: Color::Rgb(0xcd, 0xd6, 0xf4),         // #cdd6f4
            bg: Color::Rgb(0x1e, 0x1e, 0x2e),         // #1e1e2e
            highlight_fg: Color::Rgb(0xf5, 0xe0, 0xdc), // #f5e0dc
            highlight_bg: Color::Rgb(0x31, 0x32, 0x44), // #313244
            match_fg: Color::Rgb(0xf3, 0x8b, 0xa8),   // #f38ba8 (pink)
            match_bg: Color::Reset,
            selected_bg: Color::Rgb(0x45, 0x45, 0x55), // #454555
            border_fg: Color::Rgb(0x58, 0x5b, 0x70), // #585b70
            prompt_fg: Color::Rgb(0xa6, 0xe3, 0xa1), // #a6e3a1 (green)
            status_fg: Color::Rgb(0xa6, 0xad, 0xc8), // #a6adc8
            status_bg: Color::Rgb(0x18, 0x18, 0x25), // #181825
            spinner_fg: Color::Rgb(0xfa, 0xb3, 0x87), // #fab387 (peach)
        }
    }

    pub fn style_fg(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn style_highlight(&self) -> Style {
        Style::default()
            .fg(self.highlight_fg)
            .bg(self.highlight_bg)
            .add_modifier(Modifier::BOLD)
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

    pub fn style_spinner(&self) -> Style {
        Style::default().fg(self.spinner_fg).bg(self.bg)
    }
}
