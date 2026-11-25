//! Colours and glyphs. Kept in one place so the look can be tuned without
//! touching the renderer.

use hubcodes_core::model::Status;
use ratatui::style::{Color, Modifier, Style};

pub const GRID_STEP_X: i32 = 6;
pub const GRID_STEP_Y: i32 = 3;

pub fn grid() -> Style {
    Style::default().fg(Color::Indexed(236))
}

pub fn edge(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Indexed(110))
    } else {
        Style::default().fg(Color::Indexed(240))
    }
}

pub fn edge_label() -> Style {
    Style::default()
        .fg(Color::Indexed(245))
        .add_modifier(Modifier::ITALIC)
}

pub fn pulse() -> Style {
    Style::default()
        .fg(Color::Indexed(221))
        .add_modifier(Modifier::BOLD)
}

pub fn status(s: Status) -> Style {
    let c = match s {
        Status::Idle => Color::Indexed(244),
        Status::Working => Color::Indexed(114),
        Status::Waiting => Color::Indexed(221),
        Status::Error => Color::Indexed(203),
        Status::Offline => Color::Indexed(238),
    };
    Style::default().fg(c)
}

/// Edge-of-screen marker for an off-screen agent that needs you: the
/// "waiting" yellow, bold, on the bar grey so it reads over the grid.
pub fn needs_you_marker() -> Style {
    status(Status::Waiting).bg(Color::Indexed(234)).add_modifier(Modifier::BOLD)
}

pub fn node_border(selected: bool, provider: Color) -> Style {
    if selected {
        Style::default().fg(provider).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Indexed(240))
    }
}

pub fn node_title(provider: Color) -> Style {
    Style::default().fg(provider).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(Color::Indexed(243))
}

pub fn text() -> Style {
    Style::default().fg(Color::Indexed(252))
}

pub fn accent() -> Style {
    Style::default()
        .fg(Color::Indexed(110))
        .add_modifier(Modifier::BOLD)
}

pub fn key() -> Style {
    Style::default()
        .fg(Color::Indexed(232))
        .bg(Color::Indexed(250))
        .add_modifier(Modifier::BOLD)
}

pub fn bar() -> Style {
    Style::default().bg(Color::Indexed(234)).fg(Color::Indexed(250))
}

pub fn popup() -> Style {
    Style::default().bg(Color::Indexed(235)).fg(Color::Indexed(252))
}

const GUEST_COLORS: [u8; 5] = [213, 81, 150, 209, 123];

pub fn guest(idx: usize) -> Color {
    Color::Indexed(GUEST_COLORS[idx % GUEST_COLORS.len()])
}
