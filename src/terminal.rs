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
        self.buffer.clear();
        for (y, row) in grid.rows().enumerate() {
            queue!(self.buffer, MoveTo(0, y as u16))?;
            self.buffer.extend_from_slice(row);
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
