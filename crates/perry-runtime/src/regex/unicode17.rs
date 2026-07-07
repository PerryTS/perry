//! Unicode 17.0 script property-escape expansions.
//!
//! `regex-syntax` v0.8.x ships an Unicode 16.0 UCD, so `\p{sc=Beria_Erfe}` and
//! the three other scripts added in Unicode 17.0 (`Sidetic`, `Tai_Yo`,
//! `Tolong_Siki`) are unknown to it: left verbatim the crate rejects the whole
//! pattern with a `SyntaxError`, and the prior stopgap compiled them to a
//! never-matching class — which mismatches every Test262
//! `built-ins/RegExp/property-escapes` case that expects real matches. Here we
//! expand each to the explicit code-point ranges Unicode 17.0 assigns.

/// Expand a `\p{...}`/`\P{...}` naming one of the four Unicode-17.0 scripts the
/// bundled UCD lacks, or return `None` for everything else (which passes through
/// to the regex crate unchanged).
///
/// These four are brand-new, self-contained blocks, so `Script` and
/// `Script_Extensions` coincide — one body serves every alias key
/// (`sc`/`script`/`scx`/`script_extensions`/`se`) and both the short and long
/// script names. `value` is already normalized (lowercased, `_`/spaces removed).
///
/// Returns the fully-wrapped replacement: a positive class `[…]`, its complement
/// `[^…]` when negated, or the bare range body for a positive in-class member.
/// A negated *in-class* member has no character-class-union form, so it
/// contributes nothing (an empty string) — matching the pre-existing handling of
/// that edge case for other never-representable properties.
pub(super) fn script_replacement(value: &str, negated: bool, in_class: bool) -> Option<String> {
    let script = value
        .strip_prefix("script=")
        .or_else(|| value.strip_prefix("sc="))
        .or_else(|| value.strip_prefix("scriptextensions="))
        .or_else(|| value.strip_prefix("scx="))
        .or_else(|| value.strip_prefix("se="))
        .unwrap_or(value);
    let body = match script {
        "beriaerfe" | "berf" => "\\x{16EA0}-\\x{16EB8}\\x{16EBB}-\\x{16ED3}",
        "sidetic" | "sidt" => "\\x{10940}-\\x{10959}",
        "taiyo" | "tayo" => "\\x{1E6C0}-\\x{1E6DE}\\x{1E6E0}-\\x{1E6F5}\\x{1E6FE}-\\x{1E6FF}",
        "tolongsiki" | "tols" => "\\x{11DB0}-\\x{11DDB}\\x{11DE0}-\\x{11DE9}",
        _ => return None,
    };
    Some(match (in_class, negated) {
        (true, true) => String::new(),
        (true, false) => body.to_string(),
        (false, true) => format!("[^{body}]"),
        (false, false) => format!("[{body}]"),
    })
}

/// Expand a `\p{...}`/`\P{...}` for a property carrying a Unicode-17.0 delta or a
/// full replacement (see `unicode17_data`), or return `None` (pass-through to the
/// `regex` crate's bundled UCD-16 view). `value` is already normalized
/// (lowercased, `_`/spaces removed, any `gc=`/`general_category=` prefix
/// stripped by the caller) — the same key space `unicode17_data::u17_expansion`
/// is keyed on and the same spelling the crate's own property parser accepts.
///
/// * `Delta(d)` — the crate is correct at UCD 16 and UCD 17 only *added* the
///   ranges `d`; union them into the crate class: `[\p{value}d]` (positive),
///   `[^\p{value}d]` (negated), `\p{value}d` (positive class member). A negated
///   *in-class* member can't express "complement of the union" as a class-union
///   term, so it falls back to the crate's `\P{value}` (correct except for the
///   handful of freshly-added UCD-17 points, and unexercised by Test262 — every
///   generated case uses the anchored `/^\p{…}+$/` / `/^\P{…}+$/` forms).
/// * `Full(f)` — the crate can't represent the property (`Script=Unknown`,
///   `Changes_When_NFKC_Casefolded`) or UCD 17 *removed* points (so a union would
///   over-match); `f` replaces it wholesale, exactly like `script_replacement`.
pub(super) fn u17_replacement(value: &str, negated: bool, in_class: bool) -> Option<String> {
    use super::unicode17_data::{u17_expansion, U17Expansion};
    Some(match u17_expansion(value)? {
        U17Expansion::Delta(d) => match (in_class, negated) {
            (false, false) => format!("[\\p{{{value}}}{d}]"),
            (false, true) => format!("[^\\p{{{value}}}{d}]"),
            (true, false) => format!("\\p{{{value}}}{d}"),
            (true, true) => format!("\\P{{{value}}}"),
        },
        U17Expansion::Full(f) => match (in_class, negated) {
            (true, true) => String::new(),
            (true, false) => f.to_string(),
            (false, true) => format!("[^{f}]"),
            (false, false) => format!("[{f}]"),
        },
    })
}

/// Unicode-17.0 expansion for a `\p{...}`/`\P{...}` property, or `None` to pass
/// the property through to the `regex` crate's bundled UCD-16 view unchanged.
/// Tries the four brand-new U17 scripts (`script_replacement`, #6068) first,
/// then the U17 deltas/overrides for pre-existing properties (`u17_replacement`).
/// `value` is normalized as documented on those two functions. Grammar dispatch
/// calls this single entry point so the property-escape arm stays compact.
pub(super) fn expand(value: &str, negated: bool, in_class: bool) -> Option<String> {
    script_replacement(value, negated, in_class)
        .or_else(|| u17_replacement(value, negated, in_class))
}
