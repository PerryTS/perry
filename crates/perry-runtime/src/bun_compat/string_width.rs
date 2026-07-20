//! `Bun.stringWidth` parity implementation (terminal cell width).
//!
//! Faithful port of Bun v1.3.12's `src/string/immutable/visible.zig` +
//! `grapheme.zig` scalar semantics (the SIMD fast paths there are
//! optimizations only, verified equivalent):
//!
//! - Strings whose code units all fit in Latin-1 take the byte path: every
//!   printable byte (>= 0x20, not DEL/C1, not soft hyphen) is 1 cell, no
//!   East-Asian/grapheme logic (mirrors JSC 8-bit strings).
//! - Otherwise the UTF-16-semantics path runs bun's grapheme-cluster state
//!   machine (UAX #29 incl. GB9c Indic + GB11 emoji ZWJ) with per-cluster
//!   width rules (emoji sequences collapse to 2, keycaps 2, RI pairs 2,
//!   VS16 upgrades, East-Asian Wide/Fullwidth 2, ambiguous configurable).
//! - By default ANSI CSI (`ESC [ .. final`) and OSC (`ESC ] .. BEL/ST`)
//!   sequences measure 0 (unterminated ones swallow the rest); with
//!   `countAnsiEscapeCodes: true` no escape parsing happens at all and the
//!   ESC/BEL bytes simply measure 0 as control characters.
//!
//! Tables in `width_tables.rs` are generated from the pinned Bun sources —
//! see that file's header for provenance.

use super::width_tables::{GcbClass, EAW_AMBIGUOUS, EAW_WIDE, EMOJI_BASE, GCB_RANGES};

#[inline]
fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(s, e)| {
            if e < cp {
                core::cmp::Ordering::Less
            } else if s > cp {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// bun `grapheme_tables.zig` `table.get(cp)` (Hangul syllables algorithmic).
fn grapheme_class(cp: u32) -> GcbClass {
    if (0xAC00..=0xD7A3).contains(&cp) {
        return if (cp - 0xAC00) % 28 == 0 {
            GcbClass::Lv
        } else {
            GcbClass::Lvt
        };
    }
    match GCB_RANGES.binary_search_by(|&(s, e, _)| {
        if e < cp {
            core::cmp::Ordering::Less
        } else if s > cp {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => GCB_RANGES[i].2,
        Err(_) => GcbClass::Other,
    }
}

/// bun `visible.zig` `isZeroWidthCodepointType(u32, cp)`.
fn is_zero_width(cp: u32) -> bool {
    if cp <= 0x1F {
        return true;
    }
    if (0x7F..=0x9F).contains(&cp) {
        return true;
    }
    if cp == 0xAD {
        return true;
    }
    if (0x300..=0x36F).contains(&cp) {
        return true;
    }
    if (0x200B..=0x200F).contains(&cp) {
        return true;
    }
    if (0x2060..=0x2064).contains(&cp) {
        return true;
    }
    if (0x20D0..=0x20FF).contains(&cp) {
        return true;
    }
    if (0xFE00..=0xFE0F).contains(&cp) {
        return true;
    }
    if (0xFE20..=0xFE2F).contains(&cp) {
        return true;
    }
    if cp == 0xFEFF {
        return true;
    }
    if (0xD800..=0xDFFF).contains(&cp) {
        return true;
    }
    // Arabic formatting characters
    if (0x600..=0x605).contains(&cp) || cp == 0x6DD || cp == 0x70F || cp == 0x8E2 {
        return true;
    }
    // Indic script combining marks (Devanagari through Malayalam)
    if (0x900..=0xD4F).contains(&cp) {
        let offset = cp & 0x7F;
        if offset <= 0x02 {
            return true;
        }
        if (0x3A..=0x4D).contains(&offset) && offset != 0x3D {
            return true;
        }
        if (0x51..=0x57).contains(&offset) {
            return true;
        }
        if (0x62..=0x63).contains(&offset) {
            return true;
        }
    }
    // Thai combining marks
    if cp == 0xE31 || (0xE34..=0xE3A).contains(&cp) || (0xE47..=0xE4E).contains(&cp) {
        return true;
    }
    // Lao combining marks
    if cp == 0xEB1 || (0xEB4..=0xEBC).contains(&cp) || (0xEC8..=0xECD).contains(&cp) {
        return true;
    }
    if (0x1AB0..=0x1AFF).contains(&cp) {
        return true;
    }
    if (0x1DC0..=0x1DFF).contains(&cp) {
        return true;
    }
    // Tag characters
    if (0xE0000..=0xE007F).contains(&cp) {
        return true;
    }
    // Variation Selectors Supplement
    if (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }
    false
}

/// bun `visible.zig` `visibleCodepointWidthType`.
fn cp_width(cp: u32, ambiguous_as_wide: bool) -> u32 {
    if is_zero_width(cp) {
        return 0;
    }
    if cp >= 0x1100 && in_ranges(&EAW_WIDE, cp) {
        return 2;
    }
    if ambiguous_as_wide && in_ranges(&EAW_AMBIGUOUS, cp) {
        return 2;
    }
    1
}

/// bun `visible.zig` `GraphemeState.isEmojiBase` (ICU `UCHAR_EMOJI` behind
/// the same prefilters).
fn is_emoji_base(cp: u32) -> bool {
    if cp < 0x203C {
        return false;
    }
    if (0x2C00..0x1F000).contains(&cp) {
        return false;
    }
    if cp == 0xFE0E || cp == 0xFE0F || cp == 0x200D {
        return false;
    }
    in_ranges(&EMOJI_BASE, cp)
}

#[inline]
fn is_regional_indicator(cp: u32) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&cp)
}

#[inline]
fn is_skin_tone_modifier(cp: u32) -> bool {
    (0x1F3FB..=0x1F3FF).contains(&cp)
}

// ---------------------------------------------------------------------------
// Grapheme break (port of bun grapheme.zig computeGraphemeBreakNoControl)
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
enum BreakState {
    Default,
    RegionalIndicator,
    ExtendedPictographic,
    IndicConsonant,
    IndicLinker,
}

#[inline]
fn is_indic_extend(gb: GcbClass) -> bool {
    gb == GcbClass::IndicConjunctBreakExtend || gb == GcbClass::Zwj
}

#[inline]
fn is_extend(gb: GcbClass) -> bool {
    gb == GcbClass::Zwnj
        || gb == GcbClass::IndicConjunctBreakExtend
        || gb == GcbClass::IndicConjunctBreakLinker
}

#[inline]
fn is_extended_pictographic(gb: GcbClass) -> bool {
    gb == GcbClass::ExtendedPictographic || gb == GcbClass::EmojiModifierBase
}

fn grapheme_break(gb1: GcbClass, gb2: GcbClass, state: &mut BreakState) -> bool {
    use GcbClass::*;
    // Reset state when gb1/gb2 are not expected in sequence.
    match *state {
        BreakState::RegionalIndicator => {
            if gb1 != RegionalIndicator || gb2 != RegionalIndicator {
                *state = BreakState::Default;
            }
        }
        BreakState::ExtendedPictographic => {
            if !matches!(
                gb1,
                IndicConjunctBreakExtend
                    | IndicConjunctBreakLinker
                    | Zwnj
                    | Zwj
                    | ExtendedPictographic
                    | EmojiModifierBase
                    | EmojiModifier
            ) || !matches!(
                gb2,
                IndicConjunctBreakExtend
                    | IndicConjunctBreakLinker
                    | Zwnj
                    | Zwj
                    | ExtendedPictographic
                    | EmojiModifierBase
                    | EmojiModifier
            ) {
                *state = BreakState::Default;
            }
        }
        BreakState::IndicConsonant | BreakState::IndicLinker => {
            if !matches!(
                gb1,
                IndicConjunctBreakConsonant
                    | IndicConjunctBreakLinker
                    | IndicConjunctBreakExtend
                    | Zwj
            ) || !matches!(
                gb2,
                IndicConjunctBreakConsonant
                    | IndicConjunctBreakLinker
                    | IndicConjunctBreakExtend
                    | Zwj
            ) {
                *state = BreakState::Default;
            }
        }
        BreakState::Default => {}
    }

    // GB6: L x (L | V | LV | LVT)
    if gb1 == L && matches!(gb2, L | V | Lv | Lvt) {
        return false;
    }
    // GB7: (LV | V) x (V | T)
    if matches!(gb1, Lv | V) && matches!(gb2, V | T) {
        return false;
    }
    // GB8: (LVT | T) x T
    if matches!(gb1, Lvt | T) && gb2 == T {
        return false;
    }
    // GB9a: SpacingMark
    if gb2 == SpacingMark {
        return false;
    }
    // GB9b: Prepend
    if gb1 == Prepend {
        return false;
    }
    // GB9c: Indic Conjunct Break
    if gb1 == IndicConjunctBreakConsonant {
        if is_indic_extend(gb2) {
            *state = BreakState::IndicConsonant;
            return false;
        } else if gb2 == IndicConjunctBreakLinker {
            *state = BreakState::IndicLinker;
            return false;
        }
    } else if *state == BreakState::IndicConsonant {
        if gb2 == IndicConjunctBreakLinker {
            *state = BreakState::IndicLinker;
            return false;
        } else if is_indic_extend(gb2) {
            return false;
        } else {
            *state = BreakState::Default;
        }
    } else if *state == BreakState::IndicLinker {
        if gb2 == IndicConjunctBreakLinker || is_indic_extend(gb2) {
            return false;
        } else if gb2 == IndicConjunctBreakConsonant {
            *state = BreakState::Default;
            return false;
        } else {
            *state = BreakState::Default;
        }
    }

    // GB11: Emoji ZWJ sequence and Emoji modifier sequence
    if is_extended_pictographic(gb1) {
        if is_extend(gb2) || gb2 == Zwj {
            *state = BreakState::ExtendedPictographic;
            return false;
        }
        if gb1 == EmojiModifierBase && gb2 == EmojiModifier {
            *state = BreakState::ExtendedPictographic;
            return false;
        }
    } else if *state == BreakState::ExtendedPictographic {
        if (is_extend(gb1) || gb1 == EmojiModifier) && (is_extend(gb2) || gb2 == Zwj) {
            return false;
        } else if gb1 == Zwj && is_extended_pictographic(gb2) {
            *state = BreakState::Default;
            return false;
        } else {
            *state = BreakState::Default;
        }
    }

    // GB12 / GB13: Regional Indicator
    if gb1 == RegionalIndicator && gb2 == RegionalIndicator {
        if *state == BreakState::Default {
            *state = BreakState::RegionalIndicator;
            return false;
        } else {
            *state = BreakState::Default;
            return true;
        }
    }

    // GB9: x (Extend | ZWJ)
    if is_extend(gb2) || gb2 == Zwj {
        return false;
    }

    // GB999
    true
}

// ---------------------------------------------------------------------------
// Per-grapheme width (port of bun visible.zig GraphemeState)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct GraphemeState {
    first_cp: u32,
    count: u32,
    base_width: u32,
    non_emoji_width: u32,
    emoji_base: bool,
    keycap: bool,
    regional_indicator: bool,
    skin_tone: bool,
    zwj: bool,
    vs15: bool,
    vs16: bool,
}

impl GraphemeState {
    fn reset(&mut self, cp: u32, ambiguous_as_wide: bool) {
        self.first_cp = cp;
        if cp < 0x80 {
            let w = if (0x20..0x7F).contains(&cp) { 1 } else { 0 };
            *self = GraphemeState {
                first_cp: cp,
                count: 1,
                base_width: w,
                non_emoji_width: w,
                ..GraphemeState::default()
            };
            return;
        }
        let w = if is_zero_width(cp) {
            0
        } else {
            cp_width(cp, ambiguous_as_wide)
        };
        *self = GraphemeState {
            first_cp: cp,
            count: 1,
            base_width: w,
            non_emoji_width: w,
            emoji_base: is_emoji_base(cp),
            keycap: cp == 0x20E3,
            regional_indicator: is_regional_indicator(cp),
            skin_tone: is_skin_tone_modifier(cp),
            zwj: cp == 0x200D,
            ..GraphemeState::default()
        };
    }

    fn add(&mut self, cp: u32, ambiguous_as_wide: bool) {
        // count saturates at u8 in bun's packed state
        self.count = (self.count + 1).min(255);
        self.keycap = self.keycap || cp == 0x20E3;
        self.regional_indicator = self.regional_indicator || is_regional_indicator(cp);
        self.skin_tone = self.skin_tone || is_skin_tone_modifier(cp);
        self.zwj = self.zwj || cp == 0x200D;
        self.vs15 = self.vs15 || cp == 0xFE0E;
        self.vs16 = self.vs16 || cp == 0xFE0F;
        if !is_zero_width(cp) {
            // non_emoji_width saturates at u10 in bun's packed state
            self.non_emoji_width =
                (self.non_emoji_width + cp_width(cp, ambiguous_as_wide)).min(1023);
        }
    }

    fn width(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        if self.regional_indicator && self.count >= 2 {
            return 2;
        }
        if self.keycap {
            return 2;
        }
        if self.regional_indicator {
            return 1;
        }
        if self.emoji_base && (self.skin_tone || self.zwj) {
            return 2;
        }
        if self.vs15 || self.vs16 {
            if self.base_width == 2 {
                return 2;
            }
            if self.vs16 {
                let cp = self.first_cp;
                if (0x30..=0x39).contains(&cp) || cp == 0x23 || cp == 0x2A {
                    return 1;
                }
                if cp < 0x80 {
                    return 1;
                }
                return 2;
            }
            return 1;
        }
        self.non_emoji_width as usize
    }
}

// ---------------------------------------------------------------------------
// Latin-1 path (port of bun visibleLatin1Width / ...ExcludeANSIColors)
// ---------------------------------------------------------------------------

#[inline]
fn latin1_char_width(c: u32) -> usize {
    if (127..=159).contains(&c) || c < 32 || c == 0xAD {
        0
    } else {
        1
    }
}

fn width_latin1(cps: &[u32]) -> usize {
    cps.iter().map(|&c| latin1_char_width(c)).sum()
}

fn width_latin1_exclude_ansi(cps: &[u32]) -> usize {
    let mut length = 0usize;
    let mut input = cps;
    while let Some(i) = input.iter().position(|&c| c == 0x1B) {
        length += width_latin1(&input[..i]);
        input = &input[i..];
        if input.len() < 2 {
            return length;
        }
        if input[1] == u32::from(b'[') {
            // CSI: ESC [ <params> <final in 0x40..=0x7E>
            if input.len() < 3 {
                return length;
            }
            input = &input[2..];
            match input.iter().position(|&c| (0x40..=0x7E).contains(&c)) {
                Some(t) => input = &input[t + 1..],
                None => return length,
            }
        } else if input[1] == u32::from(b']') {
            // OSC: ESC ] ... (BEL | 0x9C | ESC \)
            input = &input[2..];
            loop {
                match input
                    .iter()
                    .position(|&c| c == 0x07 || c == 0x9C || c == 0x1B)
                {
                    Some(t) => {
                        let term = input[t];
                        if term == 0x07 || term == 0x9C {
                            input = &input[t + 1..];
                            break;
                        }
                        // ESC — terminates only as "ESC \"
                        if t + 1 < input.len() && input[t + 1] == u32::from(b'\\') {
                            input = &input[t + 2..];
                            break;
                        }
                        input = &input[t + 1..];
                    }
                    None => {
                        input = &input[input.len()..];
                        break;
                    }
                }
            }
        } else {
            input = &input[1..];
        }
    }
    length += width_latin1(input);
    length
}

// ---------------------------------------------------------------------------
// UTF-16-semantics path (port of bun visibleUTF16WidthFn, scalar)
// ---------------------------------------------------------------------------

fn width_utf16(cps: &[u32], exclude_ansi_colors: bool, ambiguous_as_wide: bool) -> usize {
    let mut len = 0usize;
    let mut prev_visible: Option<u32> = None;
    let mut prev_class = GcbClass::Other;
    let mut break_state = BreakState::Default;
    let mut gs = GraphemeState::default();
    let mut saw_1b = false;
    let mut saw_csi = false;
    let mut saw_osc = false;

    let mut i = 0usize;
    while i < cps.len() {
        let cp = cps[i];
        i += 1;

        if saw_csi {
            if cp < 0x80 {
                if (0x40..=0x7E).contains(&cp) {
                    saw_1b = false;
                    saw_csi = false;
                }
                // other ASCII: CSI parameter byte, consumed
            } else {
                // Non-ASCII ends the CSI sequence abnormally; not counted.
                saw_1b = false;
                saw_csi = false;
            }
            continue;
        }
        if saw_osc {
            if cp == 0x07 || cp == 0x9C {
                saw_1b = false;
                saw_osc = false;
            } else if cp == 0x1B {
                // Terminates only as the two-codepoint ST "ESC \".
                if i < cps.len() && cps[i] == u32::from(b'\\') {
                    saw_1b = false;
                    saw_osc = false;
                    i += 1;
                }
                // else: stray ESC inside OSC payload, keep scanning.
            }
            continue;
        }
        if saw_1b {
            if cp == u32::from(b'[') {
                saw_csi = true;
                continue;
            } else if cp == u32::from(b']') {
                saw_osc = true;
                continue;
            } else if cp == 0x1B {
                // Another ESC — starts a new potential sequence.
                continue;
            }
            if cp < 0x80 {
                // ESC + ASCII non-sequence: count directly, bypassing the
                // grapheme machine (bun does the same).
                len += cp_width(cp, ambiguous_as_wide) as usize;
                saw_1b = false;
                continue;
            }
            // ESC + non-ASCII: not a sequence; treat the char normally.
            saw_1b = false;
        }

        if exclude_ansi_colors && cp == 0x1B {
            saw_1b = true;
            continue;
        }

        let class = grapheme_class(cp);
        if let Some(_pv) = prev_visible {
            if grapheme_break(prev_class, class, &mut break_state) {
                len += gs.width();
                gs.reset(cp, ambiguous_as_wide);
            } else {
                gs.add(cp, ambiguous_as_wide);
            }
        } else {
            gs.reset(cp, ambiguous_as_wide);
        }
        prev_visible = Some(cp);
        prev_class = class;
    }

    len + gs.width()
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `Bun.stringWidth(input, { countAnsiEscapeCodes, ambiguousIsNarrow })`.
///
/// `cps` are the string's code points (surrogate pairs already combined;
/// lone surrogates, if representable, measure 0 like in Bun).
pub fn bun_string_width(
    cps: &[u32],
    count_ansi_escape_codes: bool,
    ambiguous_is_narrow: bool,
) -> usize {
    let all_latin1 = cps.iter().all(|&c| c <= 0xFF);
    if all_latin1 {
        // JSC 8-bit string path: no East-Asian/grapheme logic.
        if count_ansi_escape_codes {
            width_latin1(cps)
        } else {
            width_latin1_exclude_ansi(cps)
        }
    } else {
        width_utf16(cps, !count_ansi_escape_codes, !ambiguous_is_narrow)
    }
}

#[cfg(test)]
mod tests {
    use super::bun_string_width;

    fn w(s: &str) -> usize {
        let cps: Vec<u32> = s.chars().map(u32::from).collect();
        bun_string_width(&cps, false, true)
    }
    fn w_ansi(s: &str) -> usize {
        let cps: Vec<u32> = s.chars().map(u32::from).collect();
        bun_string_width(&cps, true, true)
    }
    fn w_wide(s: &str) -> usize {
        let cps: Vec<u32> = s.chars().map(u32::from).collect();
        bun_string_width(&cps, false, false)
    }

    /// All expectations below are real `Bun.stringWidth` outputs (v1.3.12).
    #[test]
    fn basics_and_latin1_path() {
        assert_eq!(w(""), 0);
        assert_eq!(w("a"), 1);
        assert_eq!(w("hello"), 5);
        assert_eq!(w("héllo"), 5); // precomposed é is Latin-1
        assert_eq!(w("\t"), 0);
        assert_eq!(w("\n"), 0);
        assert_eq!(w("a\tb"), 2);
        assert_eq!(w("\u{a0}"), 1); // NBSP printable
        assert_eq!(w("\u{ad}"), 0); // soft hyphen
                                    // Latin-1 strings ignore the East-Asian-ambiguous flag (JSC 8-bit path)
        assert_eq!(w_wide("\u{a1}"), 1);
    }

    #[test]
    fn ansi_escapes() {
        assert_eq!(w("\x1b[31mred\x1b[39m"), 3);
        assert_eq!(w_ansi("\x1b[31mred\x1b[39m"), 11); // ESC measures 0, params count
        assert_eq!(w("\x1b[2J"), 0);
        assert_eq!(w("\x1b]0;title\x07"), 0);
        assert_eq!(w("\x1b]0;title\x1b\\"), 0);
        assert_eq!(w("\x1b"), 0);
        assert_eq!(w("\x1bM"), 1); // ESC + non-CSI/OSC: only ESC is swallowed
        assert_eq!(w("\x1b[31"), 0); // unterminated CSI swallows the rest
        assert_eq!(w("\x1b[31mX"), 1);
        assert_eq!(w("\x1b]8;;http://x\x1b\\text\x1b]8;;\x1b\\"), 4); // OSC-8 link
        assert_eq!(w("\x1b[38:5:196mX"), 1);
        assert_eq!(w("\x1b[?25l"), 0);
        // UTF-16 path ANSI (string contains a non-Latin1 char)
        assert_eq!(w("\x1b[31m你\x1b[39m"), 2);
        assert_eq!(w_ansi("\x1b]0;title\x1b\\"), 9);
    }

    #[test]
    fn east_asian_and_ambiguous() {
        assert_eq!(w("你好"), 4);
        assert_eq!(w("ｈｅｌｌｏ"), 10); // fullwidth
        assert_eq!(w("ﾊﾛｰ"), 3); // halfwidth katakana
        assert_eq!(w("①"), 1);
        assert_eq!(w_wide("①"), 2);
        assert_eq!(w("α"), 1);
        assert_eq!(w_wide("α"), 2);
        assert_eq!(w("…"), 1);
        assert_eq!(w_wide("…"), 2);
    }

    #[test]
    fn emoji_sequences() {
        assert_eq!(w("\u{1f44d}"), 2); // 👍
        assert_eq!(w("\u{1f469}\u{200d}\u{1f4bb}"), 2); // ZWJ profession
        assert_eq!(
            w("\u{1f469}\u{200d}\u{1f469}\u{200d}\u{1f466}\u{200d}\u{1f466}"),
            2
        ); // family
        assert_eq!(w("\u{1f44b}\u{1f3fd}"), 2); // skin tone
        assert_eq!(w("\u{1f3fd}"), 2); // tone alone
        assert_eq!(w("a\u{1f3fd}"), 3); // tone after non-emoji stands alone
        assert_eq!(w("\u{1f3fb}\u{1f3fc}"), 4); // tone after tone: no absorb
        assert_eq!(w("\u{1f1e9}\u{1f1ea}"), 2); // flag (RI pair)
        assert_eq!(w("\u{1f1e9}"), 1); // single RI
        assert_eq!(w("\u{1f1e9}\u{1f1ea}\u{1f1eb}"), 3); // RI triple = pair + single
        assert_eq!(w("☀"), 1); // text presentation
        assert_eq!(w("☀\u{fe0f}"), 2); // VS16 upgrade
        assert_eq!(w("☀\u{fe0e}"), 1); // VS15
        assert_eq!(w("1\u{fe0f}\u{20e3}"), 2); // keycap
        assert_eq!(w("\u{203c}\u{200d}\u{1f4bb}"), 2); // BMP emoji joins ZWJ seq
        assert_eq!(w("A\u{200d}B"), 2); // ZWJ between non-emoji: no collapse
        assert_eq!(w("你\u{200d}好"), 4);
        assert_eq!(w("\u{1f469}\u{200d}A"), 3);
        assert_eq!(w("\u{1f469}\u{200d}\u{200d}\u{1f4bb}"), 4); // double ZWJ breaks join
        assert_eq!(
            w("\u{1f3f4}\u{e0067}\u{e0062}\u{e0073}\u{e0063}\u{e0074}\u{e007f}"),
            2
        ); // tag-sequence flag
        assert_eq!(w("\u{1f44d}abc"), 5);
    }

    #[test]
    fn zero_width_and_combining() {
        assert_eq!(w("e\u{301}"), 1); // combining acute
        assert_eq!(w("\u{301}"), 0);
        assert_eq!(w("你\u{301}"), 2);
        assert_eq!(w("\u{200b}"), 0); // ZWSP
        assert_eq!(w("\u{200d}"), 0); // ZWJ alone
        assert_eq!(w("\u{fe0f}"), 0); // VS16 alone
        assert_eq!(w("\u{feff}"), 0); // BOM
    }
}
