//! Terminal-column grid with complete graphemes on leading cells and
//! continuation cells for the remaining columns of wide graphemes.

use super::color::Color;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// One terminal cell.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub style: Style,
    pub(crate) grapheme: Option<Arc<str>>,
    pub(crate) width: u16,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
            grapheme: None,
            width: 1,
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
        let mut column = usize::from(col);
        for grapheme in text.graphemes(true) {
            let width = UnicodeWidthStr::width(grapheme);
            if width == 0 {
                continue;
            }
            if column + width > usize::from(self.width) {
                break;
            }
            let start = usize::from(row) * usize::from(self.width) + column;
            for index in start..start + width {
                self.clear_grapheme(index);
            }
            let mut chars = grapheme.chars();
            let ch = chars.next().unwrap();
            self.back[start] = Cell {
                ch,
                fg,
                bg,
                style,
                grapheme: chars.next().map(|_| Arc::from(grapheme)),
                width: width as u16,
            };
            for index in start + 1..start + width {
                self.back[index] = Cell {
                    fg,
                    bg,
                    style,
                    width: 0,
                    ..Cell::default()
                };
            }
            column += width;
        }
    }

    fn clear_grapheme(&mut self, index: usize) {
        let row_start = index / usize::from(self.width) * usize::from(self.width);
        let mut lead = index;
        while lead > row_start && self.back[lead].width == 0 {
            lead -= 1;
        }
        let end = lead + usize::from(self.back[lead].width);
        for cell in &mut self.back[lead..end] {
            *cell = Cell::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_text_reserves_terminal_columns() {
        let mut grid = Grid::new(6, 1);
        grid.paint_text(
            0,
            0,
            "你😀x",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        assert_eq!(grid.back[0].ch, '你');
        assert_eq!(grid.back[2].ch, '😀');
        assert_eq!(grid.back[4].ch, 'x');
    }

    #[test]
    fn wide_text_is_not_partially_painted_at_the_edge() {
        let mut grid = Grid::new(2, 1);
        grid.paint_text(
            0,
            0,
            "a你",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        assert_eq!(grid.back[0].ch, 'a');
        assert_eq!(grid.back[1], Cell::default());
    }

    #[test]
    fn painting_over_a_wide_tail_clears_its_lead() {
        let mut grid = Grid::new(3, 1);
        grid.paint_text(0, 0, "你", Color::Default, Color::Default, Style::default());
        grid.paint_text(0, 1, "x", Color::Default, Color::Default, Style::default());
        assert_eq!(grid.back[0], Cell::default());
        assert_eq!(grid.back[1].ch, 'x');
    }

    #[test]
    fn grapheme_clusters_occupy_one_lead_and_their_display_width() {
        for (text, width) in [("e\u{301}", 1), ("👩‍💻", 2), ("🇨🇳", 2), ("❤️", 2)] {
            let mut grid = Grid::new(5, 1);
            grid.paint_text(
                0,
                0,
                &format!("{text}x"),
                Color::Red,
                Color::Blue,
                Style(Style::BOLD),
            );
            assert_eq!(grid.back[0].width, width);
            assert_eq!(grid.back[0].grapheme.as_deref(), Some(text));
            for continuation in &grid.back[1..usize::from(width)] {
                assert_eq!(continuation.width, 0);
                assert_eq!(continuation.bg, Color::Blue);
                assert_eq!(continuation.style, Style(Style::BOLD));
            }
            assert_eq!(grid.back[usize::from(width)].ch, 'x');
        }
    }

    #[test]
    fn painting_over_wide_leads_and_multiple_spans_leaves_no_orphans() {
        let mut grid = Grid::new(6, 1);
        grid.paint_text(
            0,
            0,
            "你好",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        grid.paint_text(0, 1, "😀", Color::Default, Color::Default, Style::default());
        assert_eq!(grid.back[0], Cell::default());
        assert_eq!(grid.back[1].ch, '😀');
        assert_eq!(grid.back[2].width, 0);
        assert_eq!(grid.back[3], Cell::default());
        grid.paint_text(0, 1, "x", Color::Default, Color::Default, Style::default());
        assert_eq!(grid.back[2], Cell::default());
    }

    #[test]
    fn clipping_preserves_complete_graphemes_and_row_boundaries() {
        let mut grid = Grid::new(2, 2);
        grid.paint_text(
            0,
            1,
            "👩‍💻x",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        grid.paint_text(1, 0, "你", Color::Default, Color::Default, Style::default());
        assert_eq!(grid.back[1], Cell::default());
        assert_eq!(grid.back[2].ch, '你');
        grid.paint_text(1, 1, "x", Color::Default, Color::Default, Style::default());
        assert_eq!(grid.back[2], Cell::default());
        assert_eq!(grid.back[3].ch, 'x');
        let mut empty = Grid::new(0, 1);
        empty.paint_text(0, 0, "你", Color::Default, Color::Default, Style::default());
        assert!(empty.back.is_empty());
    }

    #[test]
    fn leading_zero_width_clusters_do_not_consume_a_column() {
        let mut grid = Grid::new(2, 1);
        grid.paint_text(
            0,
            0,
            "\u{301}x",
            Color::Default,
            Color::Default,
            Style::default(),
        );
        assert_eq!(grid.back[0].ch, 'x');
        assert_eq!(grid.back[1], Cell::default());
    }

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
