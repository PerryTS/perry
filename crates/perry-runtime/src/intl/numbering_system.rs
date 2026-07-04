//! `-u-nu-` (numbering system) Unicode-extension resolution for the Intl
//! service constructors. Splitting these BCP-47 tag helpers out of `intl.rs`
//! keeps that namespace module under the repository's 2,000-line gate.

/// A `numberingSystem` value is structurally valid when it is one or more
/// hyphen-separated subtags of 3–8 alphanumerics (the `type` Unicode nonterminal).
pub(super) fn is_well_formed_numbering_system(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|sub| {
            (3..=8).contains(&sub.len()) && sub.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// A numbering system is *supported* when it is the default `latn` (Latin/ASCII
/// digits, which need no transliteration table) or Perry has a digit table for
/// it. This is the set `resolvedOptions().numberingSystem` may report; other
/// (e.g. algorithmic) systems are treated as unsupported and fall back to `latn`.
pub(super) fn is_supported_numbering_system(name: &str) -> bool {
    name == "latn" || super::number_format_digits::numbering_system_digits(name).is_some()
}

/// ResolveLocale for the `nu` (numbering system) Unicode extension key
/// (ECMA-402): reconciles the requested locale's `-u-nu-` keyword with an
/// explicit `options.numberingSystem`, and returns the resolved
/// `(locale, numberingSystem)` pair. `opt_ns` is the already-validated,
/// lower-cased option value (or `None`). The resolved locale keeps `-u-nu-X`
/// only when `X` is the *supported* value actually used AND it originated from
/// the locale extension (i.e. an option that differs from a supported extension
/// drops the keyword). See NumberFormat resolved-numbering-system test262.
pub(super) fn resolve_numbering_system(locale: &str, opt_ns: Option<&str>) -> (String, String) {
    let ext_ns =
        numbering_system_from_locale(locale).filter(|ns| is_supported_numbering_system(ns));
    let opt_supported = opt_ns.filter(|ns| is_supported_numbering_system(ns));

    let (resolved_ns, keep_ext) = match (opt_supported, &ext_ns) {
        // Option present and supported: it wins; the locale keyword survives only
        // when it names the same value.
        (Some(opt), ext) => (opt.to_string(), ext.as_deref() == Some(opt)),
        // No usable option: fall back to the supported extension, else default.
        (None, Some(ext)) => (ext.clone(), true),
        (None, None) => ("latn".to_string(), false),
    };

    let resolved_locale = if keep_ext {
        with_numbering_system_keyword(locale, &resolved_ns)
    } else {
        strip_numbering_system_keyword(locale)
    };
    (resolved_locale, resolved_ns)
}

/// Extract the `-u-nu-<value>` numbering system from a (canonicalized) locale
/// string, lower-cased. Returns `None` when no `nu` keyword is present.
pub(super) fn numbering_system_from_locale(locale: &str) -> Option<String> {
    let lower = locale.to_ascii_lowercase();
    let subtags: Vec<&str> = lower.split('-').collect();
    let u = subtags.iter().position(|s| *s == "u")?;
    let mut i = u + 1;
    while i < subtags.len() {
        let key = subtags[i];
        // A keyword key is exactly two chars; everything up to the next key is its value.
        if key.len() == 2 {
            if key == "nu" {
                let mut value = String::new();
                let mut j = i + 1;
                while j < subtags.len() && subtags[j].len() != 2 {
                    if !value.is_empty() {
                        value.push('-');
                    }
                    value.push_str(subtags[j]);
                    j += 1;
                }
                return (!value.is_empty()).then_some(value);
            }
            i += 1;
            while i < subtags.len() && subtags[i].len() != 2 {
                i += 1;
            }
        } else {
            // Hit another singleton extension (e.g. `-t-`); `nu` lives only under `u`.
            break;
        }
    }
    None
}

/// Split a locale tag into `(base, u_keywords, tail_after_u)`, where
/// `u_keywords` is the ordered list of `(key, value)` pairs inside the `-u-`
/// extension and `tail` is everything from the next singleton onward (e.g. a
/// `-t-`/`-x-` sequence). Returns `None` when the tag has no `-u-` extension.
/// The `base` keeps its original canonical casing (`en-US`); the extension /
/// tail regions are lower-cased per UTS #35.
fn split_u_extension(locale: &str) -> Option<(String, Vec<(String, Vec<String>)>, String)> {
    // Preserve the base region's casing (`en-US` must not become `en-us`); only
    // the extension region is canonically lower-cased.
    let subtags: Vec<&str> = locale.split('-').collect();
    let u = subtags.iter().position(|s| s.eq_ignore_ascii_case("u"))?;
    let base = subtags[..u].join("-");
    let lower: Vec<String> = subtags.iter().map(|s| s.to_ascii_lowercase()).collect();

    let mut keywords: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = u + 1;
    let mut tail_start = subtags.len();
    while i < subtags.len() {
        let sub = lower[i].as_str();
        if sub.len() == 1 {
            // Next singleton (`t`/`x`/…) ends the `u` extension.
            tail_start = i;
            break;
        }
        // A keyword key is exactly two chars; the value runs until the next key.
        if sub.len() == 2 {
            let key = sub.to_string();
            let mut value = Vec::new();
            let mut j = i + 1;
            while j < subtags.len() && lower[j].len() != 2 && lower[j].len() != 1 {
                value.push(lower[j].clone());
                j += 1;
            }
            keywords.push((key, value));
            i = j;
        } else {
            // An `-u-` attribute (3+ chars with no preceding key). Keep it as a
            // value-less pseudo-keyword so round-tripping doesn't drop it.
            keywords.push((sub.to_string(), Vec::new()));
            i += 1;
        }
    }
    let tail = if tail_start < subtags.len() {
        lower[tail_start..].join("-")
    } else {
        String::new()
    };
    Some((base, keywords, tail))
}

/// Reassemble a locale from a `split_u_extension` decomposition, dropping the
/// `-u-` extension entirely when no keywords remain.
fn rebuild_locale(base: &str, keywords: &[(String, Vec<String>)], tail: &str) -> String {
    let mut out = base.to_string();
    if !keywords.is_empty() {
        out.push_str("-u");
        for (key, value) in keywords {
            out.push('-');
            out.push_str(key);
            for v in value {
                out.push('-');
                out.push_str(v);
            }
        }
    }
    if !tail.is_empty() {
        out.push('-');
        out.push_str(tail);
    }
    out
}

/// Remove the `-u-nu-<value>` keyword from a locale tag (dropping the whole
/// `-u-` extension if it becomes empty). A tag with no `nu` keyword is returned
/// unchanged.
fn strip_numbering_system_keyword(locale: &str) -> String {
    let Some((base, mut keywords, tail)) = split_u_extension(locale) else {
        return locale.to_string();
    };
    keywords.retain(|(key, _)| key != "nu");
    rebuild_locale(&base, &keywords, &tail)
}

/// Ensure the locale tag carries `-u-nu-<ns>` (adding a `-u-` extension if
/// absent, or replacing an existing `nu` value).
fn with_numbering_system_keyword(locale: &str, ns: &str) -> String {
    let (base, mut keywords, tail) = match split_u_extension(locale) {
        Some(parts) => parts,
        None => (locale.to_string(), Vec::new(), String::new()),
    };
    let value = vec![ns.to_string()];
    if let Some(entry) = keywords.iter_mut().find(|(key, _)| key == "nu") {
        entry.1 = value;
    } else {
        keywords.push(("nu".to_string(), value));
    }
    rebuild_locale(&base, &keywords, &tail)
}
