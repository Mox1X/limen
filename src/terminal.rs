use crate::ascii::CharGrid;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute, queue,
    style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Write};

/// Owns terminal state, including cleanup on partial initialization or unwinding.
pub struct Terminal {
    output: io::BufWriter<io::Stdout>,
    raw: bool,
    screen: bool,
    buffer: Vec<u8>,
}
impl Terminal {
    pub fn enter() -> io::Result<Self> {
        let mut terminal = Self {
            output: io::BufWriter::new(io::stdout()),
            raw: false,
            screen: false,
            buffer: Vec::new(),
        };
        terminal::enable_raw_mode()?;
        terminal.raw = true;
        terminal.screen = true;
        execute!(
            terminal.output,
            EnterAlternateScreen,
            Hide,
            DisableLineWrap,
            SetForegroundColor(Color::White),
            SetBackgroundColor(Color::Black)
        )?;
        terminal.output.flush()?;
        Ok(terminal)
    }
    pub fn present(&mut self, grid: &CharGrid) -> io::Result<()> {
        self.present_inner(grid, None)
    }
    pub fn present_with_status(&mut self, grid: &CharGrid, status: &str) -> io::Result<()> {
        self.present_inner(grid, Some(status))
    }
    fn present_inner(&mut self, grid: &CharGrid, status: Option<&str>) -> io::Result<()> {
        self.buffer.clear();
        for (y, row) in grid.rows().enumerate() {
            queue!(self.buffer, MoveTo(0, y as u16))?;
            self.buffer.extend_from_slice(row);
        }
        if let Some(status) = status {
            queue!(self.buffer, MoveTo(0, grid.height() as u16))?;
            let start = self.buffer.len();
            self.buffer.extend(
                status
                    .bytes()
                    .filter(|c| (b' '..=b'~').contains(c))
                    .take(grid.width()),
            );
            self.buffer.resize(start + grid.width(), b' ');
        }
        self.output.write_all(&self.buffer)?;
        self.output.flush()
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        if self.screen {
            let _ = execute!(
                self.output,
                ResetColor,
                EnableLineWrap,
                Show,
                LeaveAlternateScreen
            );
            let _ = self.output.flush();
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
