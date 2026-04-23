use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub match_fg: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub border_fg: Color,
    pub prompt_fg: Color,
    pub status_fg: Color,
    pub dim_fg: Color,
    pub preview_gutter_fg: Color,
    pub preview_highlight_bg: Color,
    pub preview_title_fg: Color,
    pub mode_bg: Color,
    pub mode_fg: Color,
    pub git_modified_fg: Color,
    pub git_added_fg: Color,
    pub git_untracked_fg: Color,
    pub git_deleted_fg: Color,
    pub git_renamed_fg: Color,
    pub git_ignored_fg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::terminal_respecting()
    }
}

impl Theme {
    pub fn terminal_respecting() -> Self {
        Self {
            fg: Color::Reset,
            bg: Color::Reset,
            match_fg: Color::Red,
            selected_bg: Color::DarkGray,
            selected_fg: Color::Reset,
            border_fg: Color::DarkGray,
            prompt_fg: Color::LightRed,
            status_fg: Color::DarkGray,
            dim_fg: Color::DarkGray,
            preview_gutter_fg: Color::DarkGray,
            preview_highlight_bg: Color::DarkGray,
            preview_title_fg: Color::LightMagenta,
            mode_bg: Color::Green,
            mode_fg: Color::Black,
            git_modified_fg: Color::Yellow,
            git_added_fg: Color::Green,
            git_untracked_fg: Color::Red,
            git_deleted_fg: Color::Red,
            git_renamed_fg: Color::Magenta,
            git_ignored_fg: Color::DarkGray,
        }
    }

    pub fn style_fg(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn style_match(&self) -> Style {
        Style::default()
            .fg(self.match_fg)
            .bg(self.bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn style_selected(&self) -> Style {
        Style::default().bg(self.selected_bg).fg(self.selected_fg)
    }

    pub fn style_prompt(&self) -> Style {
        Style::default().fg(self.prompt_fg).bg(self.bg)
    }

    pub fn style_status(&self) -> Style {
        Style::default().fg(self.status_fg).bg(self.bg)
    }

    pub fn style_dim(&self) -> Style {
        Style::default().fg(self.dim_fg).bg(self.bg)
    }

    pub fn style_border(&self) -> Style {
        Style::default().fg(self.border_fg).bg(self.bg)
    }

    pub fn style_preview_gutter(&self) -> Style {
        Style::default().fg(self.preview_gutter_fg).bg(self.bg)
    }

    pub fn style_preview_highlight(&self) -> Style {
        Style::default().bg(self.preview_highlight_bg).fg(self.fg)
    }

    pub fn style_mode_active(&self) -> Style {
        Style::default()
            .fg(self.mode_fg)
            .bg(self.mode_bg)
            .add_modifier(Modifier::BOLD)
    }
}
