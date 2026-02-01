//! UI module - Terminal rendering and input handling
//!
//! Uses ratatui for declarative TUI rendering and crossterm for input.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use crate::core::GameState;
use crate::types::PieceKind;

/// Convert PieceKind to display character and color
pub fn piece_style(kind: PieceKind) -> (char, Color, Color) {
    match kind {
        // (char, foreground, background)
        PieceKind::I => ('█', Color::Cyan, Color::Black),
        PieceKind::O => ('█', Color::Yellow, Color::Black),
        PieceKind::T => ('█', Color::Magenta, Color::Black),
        PieceKind::S => ('█', Color::Green, Color::Black),
        PieceKind::Z => ('█', Color::Red, Color::Black),
        PieceKind::J => ('█', Color::Blue, Color::Black),
        PieceKind::L => ('█', Color::Rgb(255, 165, 0), Color::Black), // Orange
    }
}

/// Render the game board with active piece and ghost
pub struct BoardWidget<'a> {
    pub state: &'a GameState,
    pub show_ghost: bool,
}

impl<'a> BoardWidget<'a> {
    pub fn new(state: &'a GameState) -> Self {
        Self {
            state,
            show_ghost: true,
        }
    }
}

impl<'a> Widget for BoardWidget<'a> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let board = &self.state.board;
        let board_width = board.width() as u16;
        let board_height = board.height() as u16;

        // Calculate cell size to fit in area
        let cell_width = area.width / board_width;
        let cell_height = area.height / board_height;

        // Use square cells (min of width/height)
        let cell_size = cell_width.min(cell_height).max(1);

        // Center the board in the area
        let total_width = board_width * cell_size;
        let total_height = board_height * cell_size;
        let start_x = area.x + (area.width - total_width) / 2;
        let start_y = area.y + (area.height - total_height) / 2;

        // Render board cells
        for y in 0..board_height {
            for x in 0..board_width {
                let cell = board.get(x as i8, y as i8);
                let screen_x = start_x + x * cell_size;
                let screen_y = start_y + y * cell_size;

                if let Some(Some(kind)) = cell {
                    let (ch, fg, _bg) = piece_style(kind);
                    let style = Style::default().fg(fg);
                    for dy in 0..cell_size {
                        for dx in 0..cell_size {
                            if screen_x + dx < buf.area().width && screen_y + dy < buf.area().height
                            {
                                let cell = &mut buf[(screen_x + dx, screen_y + dy)];
                                cell.set_char(ch);
                                cell.set_style(style);
                            }
                        }
                    }
                } else {
                    // Empty cell - render background
                    let style = Style::default().fg(Color::DarkGray);
                    for dy in 0..cell_size {
                        for dx in 0..cell_size {
                            if screen_x + dx < buf.area().width && screen_y + dy < buf.area().height
                            {
                                let cell = &mut buf[(screen_x + dx, screen_y + dy)];
                                cell.set_char('·');
                                cell.set_style(style);
                            }
                        }
                    }
                }
            }
        }

        // Render ghost piece
        if self.show_ghost && self.state.active.is_some() {
            let ghost_y = self.state.ghost_y().unwrap_or(0);
            let active = self.state.active.unwrap();
            let shape = active.shape();

            for &(dx, dy) in shape.iter() {
                let x = (active.x + dx) as u16;
                let y = (ghost_y + dy) as u16;

                if x < board_width && y < board_height {
                    let screen_x = start_x + x * cell_size;
                    let screen_y = start_y + y * cell_size;

                    let style = Style::default().fg(Color::Gray);
                    for cy in 0..cell_size {
                        for cx in 0..cell_size {
                            if screen_x + cx < buf.area().width && screen_y + cy < buf.area().height
                            {
                                let cell = &mut buf[(screen_x + cx, screen_y + cy)];
                                // Only draw ghost if cell is empty
                                if cell.symbol() == "·" {
                                    cell.set_char('░');
                                    cell.set_style(style);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Render active piece
        if let Some(active) = self.state.active {
            let shape = active.shape();
            for &(dx, dy) in shape.iter() {
                let x = (active.x + dx) as u16;
                let y = (active.y + dy) as u16;

                if x < board_width && y < board_height {
                    let screen_x = start_x + x * cell_size;
                    let screen_y = start_y + y * cell_size;

                    let (ch, fg, _bg) = piece_style(active.kind);
                    let style = Style::default().fg(fg).add_modifier(Modifier::BOLD);

                    for cy in 0..cell_size {
                        for cx in 0..cell_size {
                            if screen_x + cx < buf.area().width && screen_y + cy < buf.area().height
                            {
                                let cell = &mut buf[(screen_x + cx, screen_y + cy)];
                                cell.set_char(ch);
                                cell.set_style(style);
                            }
                        }
                    }
                }
            }
        }

        // Draw border around board
        let border_style = Style::default().fg(Color::White);
        for x in 0..total_width + 2 {
            if start_x + x > 0 && start_x + x < buf.area().width {
                let cell_top = &mut buf[(start_x + x - 1, start_y - 1)];
                cell_top.set_char('─');
                cell_top.set_style(border_style);
                let cell_bottom = &mut buf[(start_x + x - 1, start_y + total_height)];
                cell_bottom.set_char('─');
                cell_bottom.set_style(border_style);
            }
        }
        for y in 0..total_height {
            if start_y + y < buf.area().height {
                if start_x > 0 {
                    let cell_left = &mut buf[(start_x - 1, start_y + y)];
                    cell_left.set_char('│');
                    cell_left.set_style(border_style);
                }
                if start_x + total_width < buf.area().width {
                    let cell_right = &mut buf[(start_x + total_width, start_y + y)];
                    cell_right.set_char('│');
                    cell_right.set_style(border_style);
                }
            }
        }
        // Corners
        if start_x > 0 && start_y > 0 {
            let cell_corner = &mut buf[(start_x - 1, start_y - 1)];
            cell_corner.set_char('┌');
            cell_corner.set_style(border_style);
        }
        if start_x + total_width < buf.area().width && start_y > 0 {
            let cell_corner = &mut buf[(start_x + total_width, start_y - 1)];
            cell_corner.set_char('┐');
            cell_corner.set_style(border_style);
        }
        if start_x > 0 && start_y + total_height < buf.area().height {
            let cell_corner = &mut buf[(start_x - 1, start_y + total_height)];
            cell_corner.set_char('└');
            cell_corner.set_style(border_style);
        }
        if start_x + total_width < buf.area().width && start_y + total_height < buf.area().height {
            let cell_corner = &mut buf[(start_x + total_width, start_y + total_height)];
            cell_corner.set_char('┘');
            cell_corner.set_style(border_style);
        }
    }
}

/// Render a preview of a piece (for hold/next)
pub fn render_piece_preview(kind: PieceKind) -> String {
    let shape = match kind {
        PieceKind::I => "  ████  ",
        PieceKind::O => "  ██  \n  ██  ",
        PieceKind::T => "  █   \n ███  ",
        PieceKind::S => "  ██  \n██    ",
        PieceKind::Z => "██    \n  ██  ",
        PieceKind::J => "█     \n███   ",
        PieceKind::L => "    █ \n  ███ ",
    };
    shape.to_string()
}

/// Render the side panel with score, level, lines, hold, next
pub fn render_side_panel(state: &GameState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let block = Block::default()
        .title("INFO")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    block.render(area, buf);

    let inner = area.inner(Margin::new(2, 1));

    // Score
    let score_text = format!("Score: {}", state.score);
    Paragraph::new(score_text)
        .style(Style::default().fg(Color::Yellow))
        .render(inner, buf);

    // Level
    let level_text = format!("Level: {}", state.level);
    Paragraph::new(level_text)
        .style(Style::default().fg(Color::Cyan))
        .render(Rect::new(inner.x, inner.y + 2, inner.width, 1), buf);

    // Lines
    let lines_text = format!("Lines: {}", state.lines);
    Paragraph::new(lines_text)
        .style(Style::default().fg(Color::Green))
        .render(Rect::new(inner.x, inner.y + 4, inner.width, 1), buf);

    // Hold
    let hold_text = if let Some(hold) = state.hold {
        format!("Hold: {:?}", hold)
    } else {
        "Hold: -".to_string()
    };
    Paragraph::new(hold_text)
        .style(Style::default().fg(Color::Magenta))
        .render(Rect::new(inner.x, inner.y + 6, inner.width, 1), buf);

    // Next
    let next_text = if !state.next_queue.is_empty() {
        format!("Next: {:?}", state.next_queue[0])
    } else {
        "Next: -".to_string()
    };
    Paragraph::new(next_text)
        .style(Style::default().fg(Color::Blue))
        .render(Rect::new(inner.x, inner.y + 8, inner.width, 1), buf);

    // Controls help
    let controls = "Controls:\n←→ Move\n↑ Rotate\n↓ Soft\nSpace Hard\nC Hold\nP Pause\nQ Quit";
    Paragraph::new(controls)
        .style(Style::default().fg(Color::DarkGray))
        .render(Rect::new(inner.x, inner.y + 11, inner.width, 10), buf);
}

/// Render pause overlay
pub fn render_pause_overlay(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let popup_area = centered_rect(40, 20, area);
    Clear.render(popup_area, buf);

    let block = Block::default()
        .title("PAUSED")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    block.render(popup_area, buf);

    let text = Text::from(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  GAME PAUSED  ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Press P to resume"),
    ]);

    Paragraph::new(text)
        .alignment(Alignment::Center)
        .render(popup_area.inner(Margin::new(2, 1)), buf);
}

/// Render game over overlay
pub fn render_game_over_overlay(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let popup_area = centered_rect(40, 20, area);
    Clear.render(popup_area, buf);

    let block = Block::default()
        .title("GAME OVER")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));
    block.render(popup_area, buf);

    let text = Text::from(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  GAME OVER  ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Press R to restart"),
        Line::from("Press Q to quit"),
    ]);

    Paragraph::new(text)
        .alignment(Alignment::Center)
        .render(popup_area.inner(Margin::new(2, 1)), buf);
}

/// Helper function to create a centered rect
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PieceKind;

    #[test]
    fn test_piece_style() {
        let (ch, fg, _bg) = piece_style(PieceKind::I);
        assert_eq!(ch, '█');
        assert_eq!(fg, Color::Cyan);
    }

    #[test]
    fn test_render_piece_preview() {
        let preview = render_piece_preview(PieceKind::O);
        assert!(!preview.is_empty());
    }
}
