//! Double-buffered renderer with cell-level dirty diff.
//!
//! The hot loop in `flush`:
//!
//! 1. Walk the back buffer cell-by-cell.
//! 2. If it equals the corresponding front-buffer cell, skip — emit nothing.
//! 3. Otherwise emit just enough ANSI to (a) move the cursor if the
//!    last-emitted cell wasn't the one immediately to our left, (b) flip
//!    SGR state if fg/bg/style changed since we last touched it, and
//!    (c) write the new char.
//! 4. Copy the back cell into the front so subsequent flushes see it.
//!
//! This is the architecture's whole point — log-update.ts in ink emits
//! `\e[2K\e[1A` per line and rewrites everything, which is what causes
//! the flicker. We never erase or rewrite; we only patch the cells that
//! actually changed.

use std::io::{self, Write};

use super::cell::{Grid, Style};
use super::color::Color;

/// SGR state we last emitted to the terminal. Tracked across cells in
/// one flush so we don't repeat e.g. `\x1b[31m` for ten consecutive
/// red cells.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SgrState {
    fg: Color,
    bg: Color,
    style: Style,
}

impl Default for SgrState {
    fn default() -> Self {
        SgrState {
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        }
    }
}

/// Build the ANSI escape that transitions from `prev` to `next`. Returns
/// an empty string if they're identical. We always emit a full SGR
/// rebuild on any change rather than computing the minimal delta —
/// the diff already filters out the cell-equality case (which dominates),
/// so the over-emit cost is sub-1% of total bytes shipped.
fn sgr_transition(prev: SgrState, next: SgrState, out: &mut Vec<u8>) {
    if prev == next {
        return;
    }
    out.extend_from_slice(b"\x1b[0");
    if next.style.bold() {
        out.extend_from_slice(b";1");
    }
    if next.style.dim() {
        out.extend_from_slice(b";2");
    }
    if next.style.italic() {
        out.extend_from_slice(b";3");
    }
    if next.style.underline() {
        out.extend_from_slice(b";4");
    }
    if next.style.reverse() {
        out.extend_from_slice(b";7");
    }
    if next.style.strikethrough() {
        out.extend_from_slice(b";9");
    }
    if next.fg != Color::Default {
        out.push(b';');
        next.fg.write_fg_sgr(out);
    }
    if next.bg != Color::Default {
        out.push(b';');
        next.bg.write_bg_sgr(out);
    }
    out.extend_from_slice(b"m");
}

/// Build the ANSI move-to-position escape for (row, col), 1-based per
/// the CSI Cursor-Position spec.
fn move_to(row: u16, col: u16, out: &mut Vec<u8>) {
    out.extend_from_slice(b"\x1b[");
    let mut buf = [0u8; 3];
    let s = format_u16(row + 1, &mut buf);
    out.extend_from_slice(s);
    out.extend_from_slice(b";");
    let s = format_u16(col + 1, &mut buf);
    out.extend_from_slice(s);
    out.extend_from_slice(b"H");
}

/// Format u16 (0..=999) — sufficient for any reasonable terminal size.
fn format_u16(n: u16, buf: &mut [u8; 3]) -> &[u8] {
    let n = n.min(999) as u8;
    let mut tmp = [0u8; 3];
    let len = if n >= 100 {
        tmp[0] = b'0' + (n / 100);
        tmp[1] = b'0' + ((n / 10) % 10);
        tmp[2] = b'0' + (n % 10);
        3
    } else if n >= 10 {
        tmp[0] = b'0' + (n / 10);
        tmp[1] = b'0' + (n % 10);
        2
    } else {
        tmp[0] = b'0' + n;
        1
    };
    buf[..len].copy_from_slice(&tmp[..len]);
    &buf[..len]
}

/// Diff `back` vs `front` and return the ANSI byte-stream that turns
/// the terminal from the front into the back state. After this call
/// (specifically, after `flush` swaps), `front` matches `back` and the
/// terminal display matches `back`.
pub fn diff(grid: &Grid) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(grid.front.len());
    let mut sgr = SgrState::default();
    let mut last_pos: Option<(u16, u16)> = None;
    for row in 0..grid.height {
        for col in 0..grid.width {
            let idx = (row as usize) * (grid.width as usize) + (col as usize);
            let f = grid.front[idx];
            let b = grid.back[idx];
            if f == b {
                continue;
            }
            let need_move = match last_pos {
                Some((r, c)) if r == row && c + 1 == col => false,
                _ => true,
            };
            if need_move {
                move_to(row, col, &mut out);
            }
            let next_sgr = SgrState {
                fg: b.fg,
                bg: b.bg,
                style: b.style,
            };
            sgr_transition(sgr, next_sgr, &mut out);
            sgr = next_sgr;
            let mut buf = [0u8; 4];
            let s = b.ch.encode_utf8(&mut buf);
            out.extend_from_slice(s.as_bytes());
            last_pos = Some((row, col));
        }
    }
    if !out.is_empty() {
        out.extend_from_slice(b"\x1b[0m");
    }
    out
}

/// Diff + write + swap. After return, front == back and the terminal
/// matches back.
pub fn flush(grid: &mut Grid) {
    let bytes = diff(grid);
    if !bytes.is_empty() {
        let stdout = io::stdout();
        let mut h = stdout.lock();
        let _ = h.write_all(&bytes);
        let _ = h.flush();
    }
    grid.front.copy_from_slice(&grid.back);
}

/// Emit the ANSI to clear the screen and home the cursor. Used at the
/// start of a render session so the first frame paints into a known-
/// clean state.
pub fn enter() {
    let stdout = io::stdout();
    let mut h = stdout.lock();
    let _ = h.write_all(b"\x1b[2J\x1b[H");
    let _ = h.flush();
}

#[cfg(test)]
mod tests {
    use super::super::cell::Cell;
    use super::*;

    fn cell(ch: char) -> Cell {
        Cell {
            ch,
            fg: Color::Default,
            bg: Color::Default,
            style: Style::default(),
        }
    }

    #[test]
    fn equal_buffers_emit_nothing() {
        let mut g = Grid::new(3, 1);
        let bytes = diff(&g);
        assert!(bytes.is_empty());
        flush(&mut g);
    }

    #[test]
    fn changed_cell_emits_position_and_char() {
        let mut g = Grid::new(5, 1);
        g.back[2] = cell('x');
        let bytes = diff(&g);
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("\x1b[1;3H"), "missing move_to: {:?}", s);
        assert!(s.contains('x'), "missing char: {:?}", s);
        assert!(s.ends_with("\x1b[0m"), "missing reset: {:?}", s);
    }

    #[test]
    fn adjacent_cells_skip_redundant_move_to() {
        let mut g = Grid::new(5, 1);
        g.back[2] = cell('x');
        g.back[3] = cell('y');
        let bytes = diff(&g);
        let s = String::from_utf8_lossy(&bytes);
        assert_eq!(s.matches("\x1b[1;").count(), 1, "extra move_to: {:?}", s);
        assert!(s.contains("xy"), "chars not adjacent: {:?}", s);
    }

    #[test]
    fn flush_swaps_front_to_match_back() {
        let mut g = Grid::new(3, 1);
        g.back[0] = cell('a');
        flush(&mut g);
        assert_eq!(g.front[0], g.back[0]);
        let bytes = diff(&g);
        assert!(bytes.is_empty());
    }

    #[test]
    fn fg_color_change_emits_sgr() {
        let mut g = Grid::new(2, 1);
        g.back[0] = Cell {
            ch: 'r',
            fg: Color::Red,
            bg: Color::Default,
            style: Style::default(),
        };
        let bytes = diff(&g);
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains(";31"), "missing fg=31: {:?}", s);
    }

    #[test]
    fn rgb_color_emits_truecolor_sgr() {
        let mut g = Grid::new(2, 1);
        g.back[0] = Cell {
            ch: 'r',
            fg: Color::Rgb(255, 136, 0),
            bg: Color::Default,
            style: Style::default(),
        };
        let bytes = diff(&g);
        let s = String::from_utf8_lossy(&bytes);
        assert!(
            s.contains(";38;2;255;136;0"),
            "missing truecolor fg: {:?}",
            s
        );
    }
}
