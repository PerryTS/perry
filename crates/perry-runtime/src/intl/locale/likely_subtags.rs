//! A curated subset of the CLDR likely-subtags data, enough to drive
//! `Intl.Locale.prototype.maximize` / `minimize` for the common languages.
//!
//! Full likely-subtags resolution needs `icu_locale` plus its CLDR data pack,
//! which Perry does not bundle (size). This table covers the languages tested
//! by the bulk of the ECMA-402 `Locale` suite; an unknown language falls back
//! to the identity transform (no subtags added or removed), which keeps
//! `maximize`/`minimize` total and side-effect-free rather than wrong.

use super::ParsedLocale;

/// `language -> (script, region)` — the maximal expansion of a bare language.
const LANG: &[(&str, &str, &str)] = &[
    ("en", "Latn", "US"),
    ("es", "Latn", "ES"),
    ("fr", "Latn", "FR"),
    ("de", "Latn", "DE"),
    ("it", "Latn", "IT"),
    ("pt", "Latn", "BR"),
    ("nl", "Latn", "NL"),
    ("sv", "Latn", "SE"),
    ("da", "Latn", "DK"),
    ("no", "Latn", "NO"),
    ("nb", "Latn", "NO"),
    ("nn", "Latn", "NO"),
    ("fi", "Latn", "FI"),
    ("is", "Latn", "IS"),
    ("ru", "Cyrl", "RU"),
    ("uk", "Cyrl", "UA"),
    ("pl", "Latn", "PL"),
    ("cs", "Latn", "CZ"),
    ("sk", "Latn", "SK"),
    ("sl", "Latn", "SI"),
    ("hr", "Latn", "HR"),
    ("sr", "Cyrl", "RS"),
    ("bg", "Cyrl", "BG"),
    ("ro", "Latn", "RO"),
    ("hu", "Latn", "HU"),
    ("el", "Grek", "GR"),
    ("tr", "Latn", "TR"),
    ("ar", "Arab", "EG"),
    ("he", "Hebr", "IL"),
    ("fa", "Arab", "IR"),
    ("ur", "Arab", "PK"),
    ("hi", "Deva", "IN"),
    ("bn", "Beng", "BD"),
    ("ta", "Taml", "IN"),
    ("te", "Telu", "IN"),
    ("ml", "Mlym", "IN"),
    ("kn", "Knda", "IN"),
    ("mr", "Deva", "IN"),
    ("gu", "Gujr", "IN"),
    ("pa", "Guru", "IN"),
    ("th", "Thai", "TH"),
    ("lo", "Laoo", "LA"),
    ("km", "Khmr", "KH"),
    ("my", "Mymr", "MM"),
    ("vi", "Latn", "VN"),
    ("id", "Latn", "ID"),
    ("ms", "Latn", "MY"),
    ("fil", "Latn", "PH"),
    ("ja", "Jpan", "JP"),
    ("ko", "Kore", "KR"),
    ("zh", "Hans", "CN"),
    ("yue", "Hant", "HK"),
    ("af", "Latn", "ZA"),
    ("sw", "Latn", "TZ"),
    ("am", "Ethi", "ET"),
    ("ha", "Latn", "NG"),
    ("yo", "Latn", "NG"),
    ("ig", "Latn", "NG"),
    ("zu", "Latn", "ZA"),
    ("ca", "Latn", "ES"),
    ("eu", "Latn", "ES"),
    ("gl", "Latn", "ES"),
    ("cy", "Latn", "GB"),
    ("ga", "Latn", "IE"),
    ("gd", "Latn", "GB"),
    ("sq", "Latn", "AL"),
    ("mk", "Cyrl", "MK"),
    ("et", "Latn", "EE"),
    ("lv", "Latn", "LV"),
    ("lt", "Latn", "LT"),
    ("be", "Cyrl", "BY"),
    ("ka", "Geor", "GE"),
    ("hy", "Armn", "AM"),
    ("az", "Latn", "AZ"),
    ("kk", "Cyrl", "KZ"),
    ("ky", "Cyrl", "KG"),
    ("uz", "Latn", "UZ"),
    ("tg", "Cyrl", "TJ"),
    ("mn", "Cyrl", "MN"),
    ("ne", "Deva", "NP"),
    ("si", "Sinh", "LK"),
    ("und", "Latn", "US"),
];

/// `(language, region) -> script` overrides where the region disambiguates the
/// script (e.g. `zh-TW` is `Hant`, not the bare-`zh` default `Hans`).
const LANG_REGION: &[(&str, &str, &str)] = &[
    ("zh", "TW", "Hant"),
    ("zh", "HK", "Hant"),
    ("zh", "MO", "Hant"),
    ("zh", "CN", "Hans"),
    ("zh", "SG", "Hans"),
    ("pa", "PK", "Arab"),
    ("sr", "BA", "Cyrl"),
];

/// `script -> language` — the most likely language for a script, used to fill a
/// `und`-language tag during maximization.
const SCRIPT_LANG: &[(&str, &str)] = &[
    ("Latn", "en"),
    ("Cyrl", "ru"),
    ("Arab", "ar"),
    ("Grek", "el"),
    ("Hebr", "he"),
    ("Deva", "hi"),
    ("Hans", "zh"),
    ("Hant", "zh"),
    ("Jpan", "ja"),
    ("Kore", "ko"),
    ("Thai", "th"),
    ("Ethi", "am"),
    ("Armn", "hy"),
    ("Geor", "ka"),
    ("Taml", "ta"),
    ("Beng", "bn"),
];

fn lang_defaults(lang: &str) -> Option<(&'static str, &'static str)> {
    LANG.iter()
        .find(|(l, _, _)| *l == lang)
        .map(|(_, s, r)| (*s, *r))
}

fn script_for_region(lang: &str, region: &str) -> Option<&'static str> {
    LANG_REGION
        .iter()
        .find(|(l, r, _)| *l == lang && *r == region)
        .map(|(_, _, s)| *s)
}

fn lang_for_script(script: &str) -> Option<&'static str> {
    SCRIPT_LANG
        .iter()
        .find(|(s, _)| *s == script)
        .map(|(_, l)| *l)
}

/// Fully expand `(language, script, region)`, filling missing script/region from
/// the table. Returns the (possibly unchanged) maximal triple.
fn maximize_triple(
    language: &str,
    script: Option<String>,
    region: Option<String>,
) -> (String, Option<String>, Option<String>) {
    // Resolve a `und` language using the script (then the default mapping).
    let mut lang = language.to_string();
    if lang == "und" {
        if let Some(s) = &script {
            if let Some(l) = lang_for_script(s) {
                lang = l.to_string();
            }
        }
        if lang == "und" {
            lang = "en".to_string();
        }
    }

    match (script, region) {
        (Some(s), Some(r)) => (lang, Some(s), Some(r)),
        (None, Some(r)) => {
            let s = script_for_region(&lang, &r)
                .map(str::to_string)
                .or_else(|| lang_defaults(&lang).map(|(s, _)| s.to_string()));
            (lang, s, Some(r))
        }
        (Some(s), None) => {
            let r = lang_defaults(&lang).map(|(_, r)| r.to_string());
            (lang, Some(s), r)
        }
        (None, None) => match lang_defaults(&lang) {
            Some((s, r)) => (lang, Some(s.to_string()), Some(r.to_string())),
            None => (lang, None, None),
        },
    }
}

/// `Intl.Locale.prototype.maximize`: add the most likely script and region.
pub(super) fn maximize(p: &mut ParsedLocale) {
    let (lang, script, region) = maximize_triple(&p.language, p.script.clone(), p.region.clone());
    p.language = lang;
    p.script = script;
    p.region = region;
}

/// `Intl.Locale.prototype.minimize`: remove script/region that the
/// likely-subtags expansion would re-add. Chooses the shortest base subtags
/// whose maximization round-trips to the same maximal triple.
pub(super) fn minimize(p: &mut ParsedLocale) {
    let max = maximize_triple(&p.language, p.script.clone(), p.region.clone());
    // Minimization operates on the fully-resolved tag, so the result language is
    // always the maximal language (e.g. `und-Latn` minimizes to `en`).
    let lang = max.0.clone();
    p.language = lang.clone();

    // 1. language alone.
    if maximize_triple(&lang, None, None) == max {
        p.script = None;
        p.region = None;
        return;
    }
    // 2. language + region.
    if max.2.is_some() && maximize_triple(&lang, None, max.2.clone()) == max {
        p.script = None;
        p.region = max.2.clone();
        return;
    }
    // 3. language + script.
    if max.1.is_some() && maximize_triple(&lang, max.1.clone(), None) == max {
        p.script = max.1.clone();
        p.region = None;
        return;
    }
    // 4. keep the full maximal triple.
    p.script = max.1;
    p.region = max.2;
}
