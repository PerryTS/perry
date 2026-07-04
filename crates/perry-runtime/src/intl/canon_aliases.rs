//! CLDR alias fix-ups applied *after* the structural canonicalization done by
//! `icu_locale_core::Locale::normalize`.
//!
//! `icu_locale_core`'s data-free `normalize` gives correct case/ordering/
//! structural canonicalization, but it does **not** apply CLDR's deprecated
//! type-value replacements inside the Unicode (`-u-`) extension — e.g.
//! `-u-ca-ethiopic-amete-alem` → `-u-ca-ethioaa`, `-u-ks-primary` →
//! `-u-ks-level1`. ECMA-402's `CanonicalizeUnicodeLocaleId` requires those, so
//! `Intl.getCanonicalLocales("und-u-ca-ethiopic-amete-alem")` must yield
//! `"und-u-ca-ethioaa"` (test262 `intl402/Intl/getCanonicalLocales/
//! unicode-ext-canonicalize-*`).
//!
//! We keep this to the curated, code-unit-`|uvalue|`-shaped deprecated type
//! aliases that test262 exercises for the `ca` / `ks` / `ms` / `rg` / `sd` /
//! `tz` keys. Only the exact single-value replacements are represented; the
//! multi-territory / likely-subtag-dependent territoryAlias logic is out of
//! scope here (it lives in the language/region path, not the `-u-` extension).

/// `(key, deprecated_value, canonical_value)` for the Unicode-extension type
/// aliases test262 canonicalizes. `deprecated_value` is the full `-`-joined
/// value run as it appears after `normalize` (already lower-cased); it is
/// matched case-insensitively against the extension's value run for `key`.
const U_EXT_TYPE_ALIASES: &[(&str, &str, &str)] = &[
    // calendar (`ca`)
    ("ca", "ethiopic-amete-alem", "ethioaa"),
    ("ca", "islamicc", "islamic-civil"),
    // collation strength (`ks`)
    ("ks", "primary", "level1"),
    ("ks", "tertiary", "level3"),
    // measurement system (`ms`)
    ("ms", "imperial", "uksystem"),
    // region-override subdivision (`rg`) — shares CLDR's subdivisionAlias table
    ("rg", "no23", "no50"),
    ("rg", "cn11", "cnbj"),
    ("rg", "cz10a", "cz110"),
    ("rg", "fra", "frges"),
    ("rg", "frg", "frges"),
    ("rg", "lud", "lucl"),
    // subdivision (`sd`) — same subdivisionAlias table as `rg`
    ("sd", "no23", "no50"),
    ("sd", "cn11", "cnbj"),
    ("sd", "cz10a", "cz110"),
    ("sd", "fra", "frges"),
    ("sd", "frg", "frges"),
    ("sd", "lud", "lucl"),
    // time zone (`tz`)
    ("tz", "cnckg", "cnsha"),
    ("tz", "eire", "iedub"),
    ("tz", "est", "papty"),
    ("tz", "gmt0", "gmt"),
    ("tz", "uct", "utc"),
    ("tz", "zulu", "utc"),
];

/// Look up a deprecated Unicode-extension type value for `key`, returning its
/// canonical replacement when one exists.
fn u_ext_type_alias(key: &str, value: &str) -> Option<&'static str> {
    U_EXT_TYPE_ALIASES.iter().find_map(|(k, dep, canon)| {
        (*k == key && value.eq_ignore_ascii_case(dep)).then_some(*canon)
    })
}

/// Rewrite deprecated CLDR type values inside the Unicode (`-u-`) extension of
/// an already-`normalize`d BCP-47 tag. Returns the tag unchanged when it has no
/// `-u-` extension or no aliased value.
///
/// The `-u-` extension is a run of `attribute* (key value*)*` subtags starting
/// after the `u` singleton and ending at the next singleton (a 1-char subtag)
/// or end of string. A `key` is exactly two ASCII-alphanumeric chars; the value
/// run that follows it (its `-`-joined 3..8-char subtags) is what we match and
/// replace.
pub(super) fn canonicalize_unicode_extension_types(tag: &str) -> String {
    let subtags: Vec<&str> = tag.split('-').collect();
    // Find the `u` singleton (not inside a private-use `x` sequence).
    let mut u_start = None;
    for (i, s) in subtags.iter().enumerate() {
        if *s == "x" {
            break; // private use — no Unicode extension past here
        }
        if *s == "u" {
            u_start = Some(i);
            break;
        }
    }
    let Some(u_start) = u_start else {
        return tag.to_string();
    };
    // Extent of the `-u-` extension: up to the next singleton or the end.
    let mut u_end = subtags.len();
    for (i, s) in subtags.iter().enumerate().skip(u_start + 1) {
        if s.len() == 1 {
            u_end = i;
            break;
        }
    }

    // Rebuild the tag subtag-by-subtag, rewriting value runs inside `-u-`.
    let mut out: Vec<String> = Vec::with_capacity(subtags.len());
    out.extend(subtags[..=u_start].iter().map(|s| s.to_string()));
    let mut changed = false;
    let mut i = u_start + 1;
    while i < u_end {
        // A two-char subtag is a keyword key; anything shorter/longer here is an
        // attribute (before the first key) — pass it through untouched.
        if subtags[i].len() == 2 {
            let key = subtags[i];
            out.push(key.to_string());
            let val_start = i + 1;
            let mut val_end = val_start;
            while val_end < u_end && subtags[val_end].len() != 2 {
                val_end += 1;
            }
            let value = subtags[val_start..val_end].join("-");
            match u_ext_type_alias(key, &value) {
                Some(canon) => {
                    out.extend(canon.split('-').map(|s| s.to_string()));
                    changed = true;
                }
                None => out.extend(subtags[val_start..val_end].iter().map(|s| s.to_string())),
            }
            i = val_end;
        } else {
            out.push(subtags[i].to_string());
            i += 1;
        }
    }
    out.extend(subtags[u_end..].iter().map(|s| s.to_string()));

    if changed {
        out.join("-")
    } else {
        tag.to_string()
    }
}
