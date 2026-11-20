//! Shared SpecialCasing probe; the existing fallback is unchanged.
use crate::feature_hooks::Hook;
static CCC: Hook<fn(char) -> u8> = Hook::empty();

/// Canonical Combining Class of `ch`. The full UCD table lives in
/// `unicode-normalization`; with no install we fall back to a tiny table
/// covering the marks the SpecialCasing conditions actually inspect (dot above
/// / grave = 230, dot below / oblique-stroke = 220). Everything unlisted is 0,
/// which is the correct default for the base letters these rules operate on.
#[inline]
pub(crate) fn ccc(ch: char) -> u8 {
    match CCC.get() {
        Some(f) => f(ch),
        None => ccc_fallback(ch),
    }
}

fn ccc_fallback(ch: char) -> u8 {
    match ch {
        // Above (230)
        '\u{0300}'..='\u{0314}'
        | '\u{033D}'..='\u{0344}'
        | '\u{0346}'
        | '\u{034A}'..='\u{034C}'
        | '\u{0350}'..='\u{0352}'
        | '\u{0357}'
        | '\u{035B}'
        | '\u{0363}'..='\u{036F}'
        | '\u{1D185}'..='\u{1D189}' => 230,
        // Below (220)
        '\u{0316}'..='\u{0319}'
        | '\u{031C}'..='\u{0320}'
        | '\u{0323}'..='\u{0333}'
        | '\u{0339}'..='\u{033C}'
        | '\u{0347}'..='\u{0349}'
        | '\u{034D}'..='\u{034E}'
        | '\u{0353}'..='\u{0356}'
        | '\u{101FD}' => 220,
        _ => 0,
    }
}
pub(crate) fn install() {
    CCC.set(unicode_normalization::char::canonical_combining_class);
}
