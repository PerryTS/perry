//! Packed cell grid. Each cell is 16 bytes (char=4, fg=4, bg=4, style=1,
//! plus padding) now that `Color` carries truecolor RGB (see color.rs;
//! grew from 1 byte/color to 4 bytes/color), so an 80x24 terminal fits
//! in ~30 KB and a 200x80 one in ~250 KB — still well within L2.

use super::color::Color;

/// One terminal cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        }
    }
}

/// Bitmask of style flags. Bold/italic/underline/reverse are the
/// most commonly emitted SGR attributes; ink users also expect
/// `dimColor` (SGR 2 / faint) and `strikethrough` (SGR 9).
/// Added by #679 Phase 5 (ink-shape Text style parity).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Style(pub u8);

impl Style {
    pub const BOLD: u8 = 0b0000_0001;
    pub const ITALIC: u8 = 0b0000_0010;
    pub const UNDERLINE: u8 = 0b0000_0100;
    pub const REVERSE: u8 = 0b0000_1000;
    pub const DIM: u8 = 0b0001_0000;
    pub const STRIKETHROUGH: u8 = 0b0010_0000;

    pub fn bold(self) -> bool {
        (self.0 & Self::BOLD) != 0
    }
    pub fn italic(self) -> bool {
        (self.0 & Self::ITALIC) != 0
    }
    pub fn underline(self) -> bool {
        (self.0 & Self::UNDERLINE) != 0
    }
    pub fn reverse(self) -> bool {
        (self.0 & Self::REVERSE) != 0
    }
    pub fn dim(self) -> bool {
        (self.0 & Self::DIM) != 0
    }
    pub fn strikethrough(self) -> bool {
        (self.0 & Self::STRIKETHROUGH) != 0
    }
}

/// Two-buffer cell grid. `front` is what the terminal currently shows;
/// `back` is what we want it to show. `render::flush` diffs the two and
/// emits the minimum ANSI to reconcile, then swaps.
///
/// Both buffers are flat row-major Vec<Cell> — the cell at (row, col)
/// is at index `row * width + col`. `width` and `height` are recomputed
/// at every flush from the current terminal size.
pub struct Grid {
    pub width: u16,
    pub height: u16,
    pub front: Vec<Cell>,
    pub back: Vec<Cell>,
}

impl Grid {
    pub fn new(width: u16, height: u16) -> Self {
        let n = (width as usize) * (height as usize);
        Grid {
            width,
            height,
            front: vec![Cell::default(); n],
            back: vec![Cell::default(); n],
        }
    }

    /// Resize both buffers in place. Old contents are dropped — the
    /// next paint pass starts from a known-clean state.
    pub fn resize(&mut self, width: u16, height: u16) {
        if width == self.width && height == self.height {
            return;
        }
        let n = (width as usize) * (height as usize);
        self.front = vec![Cell::default(); n];
        self.back = vec![Cell::default(); n];
        self.width = width;
        self.height = height;
    }

    /// Clear the back buffer. Called at the start of every paint pass.
    pub fn clear_back(&mut self) {
        for c in self.back.iter_mut() {
            *c = Cell::default();
        }
    }

    /// Paint `text` into the back buffer starting at (row, col). Stops
    /// at the right edge; doesn't wrap. Out-of-bounds rows are silently
    /// dropped.
    pub fn paint_text(
        &mut self,
        row: u16,
        col: u16,
        text: &str,
        fg: Color,
        bg: Color,
        style: Style,
    ) {
        if row >= self.height {
            return;
        }
        let mut c = col;
        for ch in text.chars() {
            if c >= self.width {
                break;
            }
            let idx = (row as usize) * (self.width as usize) + (c as usize);
            self.back[idx] = Cell { ch, fg, bg, style };
            c += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_text_fits() {
        let mut g = Grid::new(10, 2);
        g.paint_text(
            0,
            0,
            "hello",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        assert_eq!(g.back[0].ch, 'h');
        assert_eq!(g.back[1].ch, 'e');
        assert_eq!(g.back[4].ch, 'o');
        // Untouched cells stay default.
        assert_eq!(g.back[5].ch, ' ');
    }

    #[test]
    fn paint_text_clips_at_right_edge() {
        let mut g = Grid::new(3, 1);
        g.paint_text(
            0,
            0,
            "abcdef",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        // Only 3 columns wide → only "abc" lands.
        assert_eq!(g.back[0].ch, 'a');
        assert_eq!(g.back[1].ch, 'b');
        assert_eq!(g.back[2].ch, 'c');
    }

    #[test]
    fn paint_text_skips_out_of_bounds_row() {
        let mut g = Grid::new(5, 2);
        g.paint_text(
            99,
            0,
            "hi",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        // Nothing written — all cells still default.
        assert!(g.back.iter().all(|c| c.ch == ' '));
    }

    #[test]
    fn resize_clears_buffers() {
        let mut g = Grid::new(2, 2);
        g.paint_text(0, 0, "ab", Color::Default, Color::Default, Style::default());
        g.resize(3, 3);
        assert_eq!(g.width, 3);
        assert_eq!(g.height, 3);
        assert_eq!(g.back.len(), 9);
        assert!(g.back.iter().all(|c| c.ch == ' '));
    }
}
