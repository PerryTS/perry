//! The leading units every match of a pattern consumes, derived once from its
//! source when the data cell is published, and the pre-check that reads them.
//!
//! `lead` is a short run of ASCII sets: a match begins with one unit of the
//! first set, followed by one unit of the second, and so on. It describes the
//! compiled pattern, like the flags beside it; nothing is learned at run time.
//! A subject in which no position starts that run cannot match from any start,
//! so RegExpBuiltinExec fails there before binding the program or the subject.
//!
//! The derivation reads only a small, unambiguous prefix of the source and
//! stops at the first construct it does not model; a shorter run (down to
//! none) only makes the pre-check admit more subjects, never fewer. It never
//! decides validity: it runs after Perex accepted the pattern.
//!
//! The run is ASCII only. In valid WTF-8 an ASCII unit is exactly one byte
//! below 0x80 and every such byte is that unit, so the run is searched in the
//! bytes. Case folding may add a non-ASCII member to a letter only under
//! `u`/`v`, and only for `k` (U+212A KELVIN SIGN) and `s` (U+017F LONG S); the
//! non-Unicode Canonicalize never maps a non-ASCII unit to an ASCII one. Such
//! a term ends the run.

/// Terms the run holds at most.
pub(crate) const LEAD_MAX: usize = 4;

/// An ASCII set: bit `b` of the 128 is unit `b`.
pub(crate) type AsciiSet = [u64; 2];

const DIGITS: AsciiSet = [0x03ff_0000_0000_0000, 0];

#[inline(always)]
fn has(set: &AsciiSet, byte: u8) -> bool {
    byte < 128 && (set[(byte >> 6) as usize] >> (byte & 63)) & 1 != 0
}

fn add(set: &mut AsciiSet, byte: u8) {
    set[(byte >> 6) as usize] |= 1 << (byte & 63);
}

/// `^$\.*+?()[]{}|` and `/`: an escape of one is that character in every mode.
fn syntax(byte: u8) -> bool {
    matches!(
        byte,
        b'^' | b'$'
            | b'\\'
            | b'.'
            | b'*'
            | b'+'
            | b'?'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'|'
            | b'/'
    )
}

enum Atom {
    Unit(u8),
    Set(AsciiSet),
}

/// One class atom at `at`: a literal ASCII unit, `\d`, or an escaped syntax
/// character or `-`. Anything else is not modelled.
fn class_atom(source: &[u8], at: usize) -> Option<(Atom, usize)> {
    match *source.get(at)? {
        b'\\' => match *source.get(at + 1)? {
            b'd' => Some((Atom::Set(DIGITS), at + 2)),
            e if syntax(e) || e == b'-' => Some((Atom::Unit(e), at + 2)),
            _ => None,
        },
        c if c < 0x80 => Some((Atom::Unit(c), at + 1)),
        _ => None,
    }
}

/// A non-negated class starting after its `[`. Ranges join two units; a
/// `-` next to `\d` is not modelled (Annex B reads it as a unit, `u` rejects).
fn class(source: &[u8], mut at: usize) -> Option<(AsciiSet, usize)> {
    if *source.get(at)? == b'^' {
        return None;
    }
    let mut set = [0u64; 2];
    loop {
        if *source.get(at)? == b']' {
            break;
        }
        let (atom, next) = class_atom(source, at)?;
        let range =
            source.get(next) == Some(&b'-') && source.get(next + 1).is_some_and(|&b| b != b']');
        match (atom, range) {
            (Atom::Unit(lo), true) => {
                let (Atom::Unit(hi), after) = class_atom(source, next + 1)? else {
                    return None;
                };
                if hi < lo {
                    return None;
                }
                for unit in lo..=hi {
                    add(&mut set, unit);
                }
                at = after;
            }
            (Atom::Set(_), true) => return None,
            (Atom::Unit(unit), false) => {
                add(&mut set, unit);
                at = next;
            }
            (Atom::Set(members), false) => {
                set[0] |= members[0];
                set[1] |= members[1];
                at = next;
            }
        }
    }
    (set != [0, 0]).then_some((set, at + 1))
}

/// One consuming term at `at`: its set and where it ends.
fn term(source: &[u8], at: usize, sets_v: bool) -> Option<(AsciiSet, usize)> {
    let mut set = [0u64; 2];
    match *source.get(at)? {
        b'\\' => match *source.get(at + 1)? {
            b'd' => return Some((DIGITS, at + 2)),
            e if syntax(e) => {
                add(&mut set, e);
                Some((set, at + 2))
            }
            _ => None,
        },
        // `v` classes have their own syntax (nesting, set operations).
        b'[' if !sets_v => class(source, at + 1),
        b'[' | b'.' | b'(' | b')' | b'|' | b'^' | b'$' | b'*' | b'+' | b'?' | b'{' | b'}'
        | b']' => None,
        c if c < 0x80 => {
            add(&mut set, c);
            Some((set, at + 1))
        }
        _ => None,
    }
}

/// Close `set` under case folding. `None` when a member folds outside ASCII.
fn fold(mut set: AsciiSet, unicode: bool) -> Option<AsciiSet> {
    for unit in b'A'..=b'Z' {
        let lower = unit + 32;
        if has(&set, unit) || has(&set, lower) {
            if unicode && matches!(lower, b'k' | b's') {
                return None;
            }
            add(&mut set, unit);
            add(&mut set, lower);
        }
    }
    Some(set)
}

/// Whether `|` occurs outside every group and class: then the leading terms
/// belong to one alternative only.
fn top_level_alternation(source: &[u8]) -> bool {
    let (mut depth, mut in_class, mut at) = (0usize, false, 0);
    while at < source.len() {
        match source[at] {
            b'\\' => at += 1,
            b']' if in_class => in_class = false,
            _ if in_class => {}
            b'[' => in_class = true,
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => return true,
            _ => {}
        }
        at += 1;
    }
    false
}

/// The run for `source` compiled with canonical `flags`, and its length.
pub(crate) fn derive(source: &[u8], flags: &str) -> ([AsciiSet; LEAD_MAX], u8) {
    let mut sets = [[0u64; 2]; LEAD_MAX];
    let sets_v = flags.contains('v');
    let unicode = sets_v || flags.contains('u');
    let ignore_case = flags.contains('i');
    if top_level_alternation(source) || (sets_v && source.contains(&b'[')) {
        return (sets, 0);
    }
    // `^` consumes nothing; the run starts after it, with or without `m`.
    let mut at = usize::from(source.first() == Some(&b'^'));
    let mut len = 0;
    while len < LEAD_MAX {
        let Some((set, next)) = term(source, at, sets_v) else {
            break;
        };
        let set = if ignore_case {
            match fold(set, unicode) {
                Some(set) => set,
                None => break,
            }
        } else {
            set
        };
        match source.get(next) {
            // The term may occur zero times, or a count follows: not required.
            Some(b'?' | b'*' | b'{') => break,
            // At least once; what follows may be this term again.
            Some(b'+') => {
                sets[len] = set;
                len += 1;
                break;
            }
            _ => {
                sets[len] = set;
                len += 1;
                at = next;
            }
        }
    }
    (sets, len as u8)
}

/// Whether no position of `bytes` starts the run, so the pattern cannot match
/// this subject from any start. `valid` says the bytes are known valid WTF-8;
/// otherwise only an all-ASCII subject is decided.
#[inline]
pub(crate) fn excludes(sets: &[AsciiSet; LEAD_MAX], len: u8, bytes: &[u8], valid: bool) -> bool {
    let k = len as usize;
    if k == 0 || (!valid && !bytes.is_ascii()) {
        return false;
    }
    if bytes.len() < k {
        return true;
    }
    let starts = &bytes[..=bytes.len() - k];
    let mut at = 0;
    while let Some(found) = first_member(&sets[0], &starts[at..]) {
        let start = at + found;
        if (1..k).all(|j| has(&sets[j], bytes[start + j])) {
            return false;
        }
        at = start + 1;
    }
    true
}

/// The index of the first byte of `bytes` in `set`. A set of one or two units
/// (a literal, or a letter under `i`) is searched eight bytes at a time.
#[inline]
fn first_member(set: &AsciiSet, bytes: &[u8]) -> Option<usize> {
    const LOW: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let members = set[0].count_ones() + set[1].count_ones();
    if members > 2 {
        return bytes.iter().position(|&b| has(set, b));
    }
    let lowest = |set: &AsciiSet| {
        if set[0] != 0 {
            set[0].trailing_zeros() as u8
        } else {
            64 + set[1].trailing_zeros() as u8
        }
    };
    let a = lowest(set);
    let mut rest = *set;
    rest[(a >> 6) as usize] &= !(1 << (a & 63));
    let b = if members == 2 { lowest(&rest) } else { a };
    let (sa, sb) = (LOW * u64::from(a), LOW * u64::from(b));
    // A zero byte of `w ^ splat(x)` is a byte equal to `x`. The test may flag
    // a byte above a true zero as well, but never misses the first one.
    let zero = |w: u64| w.wrapping_sub(LOW) & !w & HIGH;
    let mut chunks = bytes.chunks_exact(8);
    let mut base = 0;
    for chunk in &mut chunks {
        let w = u64::from_le_bytes(chunk.try_into().unwrap());
        if zero(w ^ sa) | zero(w ^ sb) != 0 {
            return chunk
                .iter()
                .position(|&byte| byte == a || byte == b)
                .map(|i| base + i);
        }
        base += 8;
    }
    chunks
        .remainder()
        .iter()
        .position(|&byte| byte == a || byte == b)
        .map(|i| base + i)
}

/// [`excludes`] for a string value, heap or inline. `false` for anything else.
#[inline]
pub(crate) fn excludes_value(data: &super::RegExpData, value: f64) -> bool {
    if data.lead_len == 0 {
        return false;
    }
    let v = crate::value::JSValue::from_bits(value.to_bits());
    if v.is_short_string() {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut scratch);
        return excludes(&data.lead, data.lead_len, &scratch[..n], false);
    }
    if v.is_string() {
        let header = v.as_string_ptr();
        if header.is_null() {
            return false;
        }
        // SAFETY: a live heap string; nothing here allocates.
        unsafe { excludes_header(data, header) }
    } else {
        false
    }
}

/// [`excludes`] for a heap string.
///
/// # Safety
/// `header` is a live heap string, and nothing collects while this runs.
#[inline]
pub(crate) unsafe fn excludes_header(
    data: &super::RegExpData,
    header: *const crate::string::StringHeader,
) -> bool {
    if data.lead_len == 0 || header.is_null() {
        return false;
    }
    let bytes = std::slice::from_raw_parts(
        (header as *const u8).add(std::mem::size_of::<crate::string::StringHeader>()),
        (*header).byte_len as usize,
    );
    let valid = (*header).flags & crate::string::STRING_FLAG_WTF8_VALIDATED != 0;
    excludes(&data.lead, data.lead_len, bytes, valid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, flags: &str) -> Vec<String> {
        let (sets, len) = derive(source.as_bytes(), flags);
        sets[..len as usize]
            .iter()
            .map(|set| {
                (0u8..128)
                    .filter(|&b| has(set, b))
                    .map(char::from)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn derives_the_qs_patterns() {
        assert_eq!(run(r"\+", "g"), ["+"]);
        assert_eq!(run("%5B", "gi"), ["%", "5", "Bb"]);
        assert_eq!(run("%5D", "gi"), ["%", "5", "Dd"]);
        assert_eq!(run(r"^\?", ""), ["?"]);
        assert_eq!(run(r"&#(\d+);", "g"), ["&", "#"]);
        assert_eq!(run("%[0-9a-f]{2}", "gi"), ["%"]);
        assert_eq!(run(r"%u[0-9a-f]{4}", "gi"), ["%", "Uu"]);
        assert_eq!(run(r"\.([^.[]+)", "g"), ["."]);
        assert_eq!(run("%2E", "g"), ["%", "2", "E"]);
    }

    #[test]
    fn stops_where_a_term_is_not_required() {
        assert!(run("a|b", "").is_empty());
        assert_eq!(run(r"x\|", ""), ["x", "|"]);
        assert_eq!(run("(a|b)c", ""), Vec::<String>::new());
        assert_eq!(run("ab(c|d)", ""), ["a", "b"]);
        assert!(run("a?b", "").is_empty());
        assert!(run("a*b", "").is_empty());
        assert!(run("a{0,1}b", "").is_empty());
        assert_eq!(run("ab+c", ""), ["a", "b"]);
        assert_eq!(run("abcdef", ""), ["a", "b", "c", "d"]);
        assert!(run(".a", "").is_empty());
        assert!(run(r"\bfoo", "").is_empty());
        assert!(run(r"\1a", "").is_empty());
        assert!(run("(?=a)a", "").is_empty());
        assert!(run("[^a]", "").is_empty());
        assert!(run("[]a", "").is_empty());
        assert!(run(r"[\d-z]", "").is_empty());
        assert!(run("é", "").is_empty());
        assert!(run("[a]", "v").is_empty());
        assert_eq!(run(r"\+", "v"), ["+"]);
        assert_eq!(run("[a-c-]x", ""), ["-abc", "x"]);
        assert_eq!(run(r"[\]\-]", ""), ["-]"]);
    }

    #[test]
    fn folding_ends_at_letters_that_fold_outside_ascii() {
        assert_eq!(run("ak", "i"), ["Aa", "Kk"]);
        assert_eq!(run("ak", "iu"), ["Aa"]);
        assert!(run("s", "iv").is_empty());
        assert_eq!(run("[a-c]", "iu"), ["ABCabc"]);
        assert_eq!(run("k", "u"), ["k"]);
    }

    #[test]
    fn first_member_finds_the_first_position_at_every_offset() {
        let sets = [
            derive(b"%", "").0[0],
            derive(b"b", "i").0[0],
            derive(b"[a-c]", "").0[0],
        ];
        for set in &sets {
            let member = (0u8..128).find(|&b| has(set, b)).unwrap();
            for n in 0..40 {
                let mut hay = vec![b'x'; n];
                assert_eq!(first_member(set, &hay), None);
                hay.push(member);
                hay.extend_from_slice(b"xx%Bbca");
                assert_eq!(first_member(set, &hay), Some(n));
            }
        }
        // The upper case of a folded letter is found as well.
        assert_eq!(first_member(&sets[1], b"xxxxxxxxxxB"), Some(10));
    }

    #[test]
    fn excludes_only_subjects_without_the_run() {
        let (sets, len) = derive(b"%5B", "gi");
        assert!(excludes(&sets, len, b"user[name]", false));
        assert!(excludes(&sets, len, b"a%20b%26c", false));
        assert!(!excludes(&sets, len, b"a%5bc", false));
        assert!(!excludes(&sets, len, b"%5B", false));
        assert!(excludes(&sets, len, b"%5", false));
        assert!(excludes(&sets, len, b"", false));
        // Non-ASCII bytes decide only when the encoding is known valid.
        assert!(!excludes(&sets, len, "h\u{e9}llo".as_bytes(), false));
        assert!(excludes(&sets, len, "h\u{e9}llo".as_bytes(), true));
        let (none, zero) = derive(b"a|b", "");
        assert!(!excludes(&none, zero, b"xyz", true));
    }
}
