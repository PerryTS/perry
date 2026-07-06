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
