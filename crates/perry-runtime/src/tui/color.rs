//! Cell colors. v0.1 supported only the 16-color ANSI palette + the
//! `Default` sentinel (don't emit SGR; let the terminal use its own
//! default). Phase 3.5 (#405) added `Rgb(r, g, b)` for 24-bit
//! truecolor — emitted as `ESC[38;2;R;G;Bm` (fg) / `ESC[48;2;R;G;Bm`
//! (bg). Cell encoding grew from 1 byte/color to 4 bytes/color
//! (1-byte discriminant + 3 RGB bytes); 80×24 cell-grid memory still
//! fits comfortably in L1.

/// Foreground / background color. Most user code uses `Default` or one
/// of the named palette variants; `Rgb` is the truecolor escape hatch
/// for hex / RGB-tuple inputs from the TS surface.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Color {
    #[default]
    Default,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
    /// 24-bit truecolor.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Palette index (0..=7 normal, 8..=15 bright) for the named
    /// variants; `None` for Default and Rgb.
    fn palette_index(self) -> Option<u8> {
        match self {
            Color::Black => Some(0),
            Color::Red => Some(1),
            Color::Green => Some(2),
            Color::Yellow => Some(3),
            Color::Blue => Some(4),
            Color::Magenta => Some(5),
            Color::Cyan => Some(6),
            Color::White => Some(7),
            Color::BrightBlack => Some(8),
            Color::BrightRed => Some(9),
            Color::BrightGreen => Some(10),
            Color::BrightYellow => Some(11),
            Color::BrightBlue => Some(12),
            Color::BrightMagenta => Some(13),
            Color::BrightCyan => Some(14),
            Color::BrightWhite => Some(15),
            _ => None,
        }
    }

    /// Foreground SGR code for palette colors (3x normal, 9x bright).
    /// Default → 39; Rgb → 38 (the prefix; full sequence emitted via
    /// `write_fg_sgr`). Kept for legacy tests.
    pub fn fg_code(self) -> u8 {
        match self.palette_index() {
            Some(i) if i < 8 => 30 + i,
            Some(i) => 90 + (i - 8),
            None => match self {
                Color::Default => 39,
                Color::Rgb(_, _, _) => 38,
                _ => 39,
            },
        }
    }

    /// Background SGR code (palette only); 38 for Rgb.
    pub fn bg_code(self) -> u8 {
        match self.palette_index() {
            Some(i) if i < 8 => 40 + i,
            Some(i) => 100 + (i - 8),
            None => match self {
                Color::Default => 49,
                Color::Rgb(_, _, _) => 48,
                _ => 49,
            },
        }
    }

    /// Write the SGR payload to set this color as foreground. Emits
    /// no leading `\x1b[` or trailing `m` — the renderer combines this
    /// with style bits + bg in a single CSI.
    pub fn write_fg_sgr(self, out: &mut Vec<u8>) {
        match self {
            Color::Default => append_u8(out, 39),
            Color::Rgb(r, g, b) => {
                out.extend_from_slice(b"38;2;");
                append_u8(out, r);
                out.push(b';');
                append_u8(out, g);
                out.push(b';');
                append_u8(out, b);
            }
            _ => append_u8(out, self.fg_code()),
        }
    }

    /// Write the SGR payload to set this color as background.
    pub fn write_bg_sgr(self, out: &mut Vec<u8>) {
        match self {
            Color::Default => append_u8(out, 49),
            Color::Rgb(r, g, b) => {
                out.extend_from_slice(b"48;2;");
                append_u8(out, r);
                out.push(b';');
                append_u8(out, g);
                out.push(b';');
                append_u8(out, b);
            }
            _ => append_u8(out, self.bg_code()),
        }
    }
}

/// Parse a TS-side color string. Recognized shapes:
///   - `""` / `"default"` → `Color::Default`
///   - `"red"`, `"bright-red"`, `"brightRed"` → palette
///   - `"#rrggbb"` / `"#rgb"` → `Color::Rgb`
///   - unknown → `Color::Default`
pub fn parse_color(s: &str) -> Color {
    let s = s.trim();
    if s.is_empty() {
        return Color::Default;
    }
    if let Some(rest) = s.strip_prefix('#') {
        return parse_hex(rest).unwrap_or(Color::Default);
    }
    match s.to_ascii_lowercase().as_str() {
        "default" => Color::Default,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "brightblack" | "bright-black" | "gray" | "grey" => Color::BrightBlack,
        "brightred" | "bright-red" => Color::BrightRed,
        "brightgreen" | "bright-green" => Color::BrightGreen,
        "brightyellow" | "bright-yellow" => Color::BrightYellow,
        "brightblue" | "bright-blue" => Color::BrightBlue,
        "brightmagenta" | "bright-magenta" => Color::BrightMagenta,
        "brightcyan" | "bright-cyan" => Color::BrightCyan,
        "brightwhite" | "bright-white" => Color::BrightWhite,
        _ => Color::Default,
    }
}

/// Parse a hex color (without the leading `#`). Accepts 3-digit
/// (`abc` → `aabbcc`) and 6-digit forms.
fn parse_hex(s: &str) -> Option<Color> {
    let bytes = s.as_bytes();
    let nybble = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    match bytes.len() {
        3 => {
            let r = nybble(bytes[0])?;
            let g = nybble(bytes[1])?;
            let b = nybble(bytes[2])?;
            Some(Color::Rgb(r * 17, g * 17, b * 17))
        }
        6 => {
            let r = nybble(bytes[0])? * 16 + nybble(bytes[1])?;
            let g = nybble(bytes[2])? * 16 + nybble(bytes[3])?;
            let b = nybble(bytes[4])? * 16 + nybble(bytes[5])?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

/// Append a u8 (0..=255) as decimal ASCII to `out`. Used by the SGR
/// emitter — avoids the `itoa` dep.
fn append_u8(out: &mut Vec<u8>, n: u8) {
    if n >= 100 {
        out.push(b'0' + (n / 100));
        out.push(b'0' + ((n / 10) % 10));
        out.push(b'0' + (n % 10));
    } else if n >= 10 {
        out.push(b'0' + (n / 10));
        out.push(b'0' + (n % 10));
    } else {
        out.push(b'0' + n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fg_codes_match_ansi_spec() {
        assert_eq!(Color::Default.fg_code(), 39);
        assert_eq!(Color::Black.fg_code(), 30);
        assert_eq!(Color::Red.fg_code(), 31);
        assert_eq!(Color::White.fg_code(), 37);
        assert_eq!(Color::BrightBlack.fg_code(), 90);
        assert_eq!(Color::BrightRed.fg_code(), 91);
        assert_eq!(Color::BrightWhite.fg_code(), 97);
    }

    #[test]
    fn bg_codes_match_ansi_spec() {
        assert_eq!(Color::Default.bg_code(), 49);
        assert_eq!(Color::Black.bg_code(), 40);
        assert_eq!(Color::Red.bg_code(), 41);
        assert_eq!(Color::White.bg_code(), 47);
        assert_eq!(Color::BrightWhite.bg_code(), 107);
    }

    #[test]
    fn rgb_fg_sgr_is_38_2_rgb() {
        let mut out = Vec::new();
        Color::Rgb(255, 136, 0).write_fg_sgr(&mut out);
        assert_eq!(&out, b"38;2;255;136;0");
    }

    #[test]
    fn rgb_bg_sgr_is_48_2_rgb() {
        let mut out = Vec::new();
        Color::Rgb(0, 0, 0).write_bg_sgr(&mut out);
        assert_eq!(&out, b"48;2;0;0;0");
    }

    #[test]
    fn parse_color_named_palette() {
        assert_eq!(parse_color("red"), Color::Red);
        assert_eq!(parse_color("RED"), Color::Red);
        assert_eq!(parse_color("bright-red"), Color::BrightRed);
        assert_eq!(parse_color("brightRed"), Color::BrightRed);
        assert_eq!(parse_color("gray"), Color::BrightBlack);
        assert_eq!(parse_color("grey"), Color::BrightBlack);
    }

    #[test]
    fn parse_color_hex_6() {
        assert_eq!(parse_color("#ff8800"), Color::Rgb(255, 136, 0));
        assert_eq!(parse_color("#000000"), Color::Rgb(0, 0, 0));
        assert_eq!(parse_color("#FFFFFF"), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn parse_color_hex_3() {
        // `#fa0` → `#ffaa00`.
        assert_eq!(parse_color("#fa0"), Color::Rgb(255, 170, 0));
    }

    #[test]
    fn parse_color_default_on_garbage() {
        assert_eq!(parse_color(""), Color::Default);
        assert_eq!(parse_color("garbage"), Color::Default);
        assert_eq!(parse_color("#xyz"), Color::Default);
        assert_eq!(parse_color("#1234"), Color::Default); // 4 hex digits
    }
}
