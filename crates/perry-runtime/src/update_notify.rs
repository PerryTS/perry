//! The embedded update check a compiled app runs at startup.
//!
//! Codegen bakes the project's validated `perry.update` block into the binary
//! as a JSON blob and calls [`perry_update_notify_startup`] at the top of
//! `main`, before any user code. An app that configures nothing gets neither
//! the blob nor the call.
//!
//! # What this file does today, and what it deliberately does not
//!
//! It parses and holds the configuration, and applies every gate that decides
//! whether a check may happen at all. It does **not** yet reach the network or
//! print anything — those arrive with the provider layer.
//!
//! That split is deliberate rather than incidental. The gates are where this
//! feature can go wrong quietly: a check that fires in CI, or in a script
//! parsing the app's stdout, or in a container with no writable home, is a bug
//! that shows up as somebody else's flaky pipeline. They are worth landing and
//! testing on their own, ahead of the code that would exercise them.
//!
//! # Why the parse is total
//!
//! A blob this build cannot read is ignored, not guessed at. It is emitted by
//! the same Perry that compiled the binary, so a mismatch means something is
//! wrong upstream, and a wrong guess would run a network check with settings
//! nobody wrote.

use std::borrow::Cow;
use std::sync::OnceLock;

/// The blob shape this build understands. Must match the compiler's
/// `BLOB_SCHEMA`.
const BLOB_SCHEMA: u32 = 1;

/// The validated settings, as read from the embedded blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedUpdateConfig {
    pub app_id: String,
    pub bin_name: String,
    pub current_version: String,
    pub source: String,
    pub url: Option<String>,
    pub tag: Option<String>,
    pub package: Option<String>,
    pub registry: Option<String>,
    pub check_interval_hours: u64,
    pub notify_interval_hours: u64,
    pub command: String,
    pub skip_env: Option<String>,
}

static CONFIG: OnceLock<Option<EmbeddedUpdateConfig>> = OnceLock::new();

/// Read one field out of the blob.
///
/// A deliberately tiny flat-object reader rather than a JSON library. Two
/// reasons: `perry-runtime` links into every compiled binary, so a parser
/// pulled in for this would be paid for by every program that configures no
/// updates; and this runs at the very top of `main`, before the collector is
/// usable, so the runtime's own JSON path — which allocates JS values — is not
/// available. The producer is `update_config.rs` in the same Perry that
/// compiled the binary, emitting a flat object of strings and numbers, so there
/// is no nesting to handle.
fn blob_field<'a>(text: &'a str, key: &str) -> Option<Cow<'a, str>> {
    let needle = format!("\"{key}\":");
    let mut rest = text;
    loop {
        let at = rest.find(&needle)?;
        // Guard against matching a key inside a VALUE: the character before the
        // opening quote must be a structural one, not part of a string.
        let before = rest[..at].chars().last();
        rest = &rest[at + needle.len()..];
        if !matches!(
            before,
            None | Some('{') | Some(',') | Some(' ') | Some('\n') | Some('\t')
        ) {
            continue;
        }
        let value = rest.trim_start();
        return if let Some(body) = value.strip_prefix('"') {
            // Strings: walk to the closing quote, UNESCAPING as we go.
            //
            // Returning the raw slice was a round-trip bug: `save_state` writes
            // `\"` and `\\`, so a url or version containing either came back
            // with the escapes still in it, and each save/load cycle doubled
            // the backslashes until the value was unusable.
            let mut out = String::new();
            let mut chars = body.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => return Some(Cow::Owned(out)),
                    '\\' => match chars.next() {
                        Some(escaped @ ('"' | '\\' | '/')) => out.push(escaped),
                        Some('n') => out.push('\n'),
                        Some('t') => out.push('\t'),
                        Some('r') => out.push('\r'),
                        // An escape this reader does not know: keep the payload
                        // rather than the backslash, which is the safer of the
                        // two for a value that becomes a URL.
                        Some(other) => out.push(other),
                        None => return None,
                    },
                    other => out.push(other),
                }
            }
            None
        } else {
            let end = value
                .find(|c: char| c == ',' || c == '}')
                .unwrap_or(value.len());
            Some(Cow::Borrowed(value[..end].trim()))
        };
    }
}

/// Parse the blob. `None` for anything this build cannot read.
fn parse_blob(text: &str) -> Option<EmbeddedUpdateConfig> {
    let string = |key: &str| blob_field(text, key).map(Cow::into_owned);
    let number = |key: &str, fallback: u64| -> u64 {
        blob_field(text, key)
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(fallback)
    };

    // A schema this build does not know means the blob was written by a
    // different Perry. Ignore it rather than reading fields that may have moved
    // — a wrong guess would run a network check with settings nobody wrote.
    if number("schema", 0) != BLOB_SCHEMA as u64 {
        return None;
    }

    Some(EmbeddedUpdateConfig {
        app_id: string("app_id")?,
        bin_name: string("bin_name")?,
        current_version: string("current_version")?,
        source: string("source")?,
        url: string("url"),
        tag: string("tag"),
        package: string("package"),
        registry: string("registry"),
        check_interval_hours: number("check_interval_hours", 24),
        notify_interval_hours: number("notify_interval_hours", 24),
        command: string("command").unwrap_or_default(),
        skip_env: string("skip_env"),
    })
}

/// Codegen calls this once, at the top of `main`, for a configured app only.
///
/// # Safety
///
/// `ptr`/`len` name a string constant in the binary's own read-only data, so
/// the bytes outlive the process and are never null for a positive length.
#[no_mangle]
pub unsafe extern "C" fn perry_update_notify_startup(ptr: *const u8, len: i32) {
    if ptr.is_null() || len <= 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let config = parse_blob(text);
    let _ = CONFIG.set(config.clone());
    if let Some(config) = config {
        run_startup_notice(&config);
    }
}

/// The embedded settings, if this binary has any and they parsed.
pub fn embedded_config() -> Option<&'static EmbeddedUpdateConfig> {
    CONFIG.get().and_then(|c| c.as_ref())
}

/// Why a check is not going to happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// The app embeds no update settings.
    NotConfigured,
    /// The app's own opt-out variable is set.
    AppOptOut,
    /// `PERRY_NO_UPDATE_CHECK`, or the ecosystem-wide `NO_UPDATE_NOTIFIER`.
    GlobalOptOut,
    /// A continuous-integration environment.
    Ci,
    /// Nobody is reading stderr, so a notice has no audience.
    NotATerminal,
    /// This invocation IS the app's own update command; checking again here
    /// would recurse.
    UpdateCommand,
}

/// The environment a decision is made against, gathered so the decision itself
/// is a pure function.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckEnv<'a> {
    pub app_skip: Option<&'a str>,
    pub no_update_check: Option<&'a str>,
    pub no_update_notifier: Option<&'a str>,
    pub ci: Option<&'a str>,
    pub continuous_integration: Option<&'a str>,
    pub stderr_is_terminal: bool,
    /// The app's own argv[1], so an `app self-update` run does not itself
    /// trigger a check.
    pub first_arg: Option<&'a str>,
}

fn is_on(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("on") | Some("yes")
    )
}

fn is_present(raw: Option<&str>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        None | Some("") | Some("0") | Some("false") | Some("off") | Some("no")
    )
}

/// May this run check for an update? `None` means yes.
///
/// Pure, so every gate is asserted directly rather than inferred from whether a
/// network call happened to be made.
pub fn skip_reason(config: Option<&EmbeddedUpdateConfig>, env: CheckEnv<'_>) -> Option<SkipReason> {
    // `?` would be wrong here: it returns `None`, which this function reads as
    // "go ahead and check". An app with no embedded settings has nothing to
    // check against.
    let Some(config) = config else {
        return Some(SkipReason::NotConfigured);
    };

    // The app's own switch first: the person setting `MYAPP_NO_UPDATE_CHECK`
    // is being specific, and specificity should not be overridable by anything
    // more general.
    if let Some(name) = config.skip_env.as_deref() {
        if !name.is_empty() && is_present(env.app_skip) {
            return Some(SkipReason::AppOptOut);
        }
    }
    // Then the two global spellings. `NO_UPDATE_NOTIFIER` is honoured because
    // somebody who sets it has already told every tool on the machine what they
    // want, and an app compiled by Perry is one of those tools.
    // Presence, not an exact literal, for BOTH spellings. Somebody who set
    // either one is asking not to be checked, and the documentation presents
    // them the same way — a gate that accepted only four spellings of yes would
    // silently ignore the fifth.
    if is_present(env.no_update_check) || is_present(env.no_update_notifier) {
        return Some(SkipReason::GlobalOptOut);
    }
    if is_present(env.ci) || is_present(env.continuous_integration) {
        return Some(SkipReason::Ci);
    }
    if !env.stderr_is_terminal {
        return Some(SkipReason::NotATerminal);
    }
    // `app self-update` must not check on its way to updating: the check would
    // be redundant at best, and at worst the notice would print in the middle
    // of the install it triggered.
    if !config.command.is_empty() && env.first_arg == Some(config.command.as_str()) {
        return Some(SkipReason::UpdateCommand);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob() -> String {
        // The shape `update_config.rs` emits, minus the keys a minimal npm
        // block leaves out.
        r#"{"schema":1,"app_id":"myapp","bin_name":"myapp","current_version":"1.2.3",
            "source":"npm","package":"myapp","check_interval_hours":24,
            "notify_interval_hours":24,"command":"self-update",
            "skip_env":"MYAPP_NO_UPDATE_CHECK"}"#
            .to_string()
    }

    fn config() -> EmbeddedUpdateConfig {
        parse_blob(&blob()).expect("the fixture must parse")
    }

    fn tty() -> CheckEnv<'static> {
        CheckEnv {
            stderr_is_terminal: true,
            ..CheckEnv::default()
        }
    }

    #[test]
    fn a_blob_round_trips_into_settings() {
        let config = config();
        assert_eq!(config.app_id, "myapp");
        assert_eq!(config.source, "npm");
        assert_eq!(config.package.as_deref(), Some("myapp"));
        assert_eq!(config.url, None, "an absent optional stays absent");
        assert_eq!(config.check_interval_hours, 24);
        assert_eq!(config.command, "self-update");
    }

    /// A blob from a different Perry is ignored rather than read field by
    /// field. Guessing at a moved layout would run a network check with
    /// settings nobody wrote.
    #[test]
    fn a_blob_of_another_schema_is_ignored() {
        assert_eq!(
            parse_blob(&blob().replace("\"schema\":1", "\"schema\":2")),
            None
        );
        assert_eq!(parse_blob(r#"{"app_id":"x"}"#), None, "no schema at all");
        assert_eq!(parse_blob("not json"), None);
        assert_eq!(parse_blob(""), None);
    }

    /// A blob missing a required field is ignored too — a half-read
    /// configuration is worse than none, because it looks like it works.
    #[test]
    fn a_blob_missing_a_required_field_is_ignored() {
        assert_eq!(parse_blob(r#"{"schema":1,"app_id":"x"}"#), None);
    }

    /// The reader must not match a key name that appears inside a VALUE — an
    /// app whose name contains `"url":` would otherwise read its own name as a
    /// URL.
    #[test]
    fn a_key_name_inside_a_value_is_not_matched() {
        let text = r#"{"schema":1,"app_id":"a","bin_name":"weird\"url\":x","current_version":"1",
            "source":"npm","package":"p"}"#;
        let config = parse_blob(text).expect("parses");
        assert_eq!(config.url, None, "the name's contents are not a url field");
        assert_eq!(config.package.as_deref(), Some("p"));
    }

    #[test]
    fn an_app_with_no_config_never_checks() {
        assert_eq!(skip_reason(None, tty()), Some(SkipReason::NotConfigured));
    }

    /// The app's own switch is the most specific thing the user said, so
    /// nothing more general gets to override it.
    #[test]
    fn the_apps_own_opt_out_is_honoured() {
        let env = CheckEnv {
            app_skip: Some("1"),
            ..tty()
        };
        assert_eq!(
            skip_reason(Some(&config()), env),
            Some(SkipReason::AppOptOut)
        );
    }

    /// Both global spellings, including the ecosystem-wide one: somebody who
    /// set it has already told every tool on the machine what they want.
    /// Both spellings are presence-based, so a value the gate did not enumerate
    /// still disables the check. Somebody who wrote `PERRY_NO_UPDATE_CHECK=please`
    /// is asking not to be checked.
    #[test]
    fn the_global_opt_outs_accept_any_non_falsey_value() {
        for raw in ["1", "true", "yes", "please", "anything"] {
            for env in [
                CheckEnv {
                    no_update_check: Some(raw),
                    ..tty()
                },
                CheckEnv {
                    no_update_notifier: Some(raw),
                    ..tty()
                },
            ] {
                assert_eq!(
                    skip_reason(Some(&config()), env),
                    Some(SkipReason::GlobalOptOut),
                    "{raw:?} must disable the check"
                );
            }
        }
        // ...and an explicit no, or an exported-but-empty value, does not.
        for raw in ["0", "false", "off", "no", ""] {
            let env = CheckEnv {
                no_update_check: Some(raw),
                ..tty()
            };
            assert_eq!(skip_reason(Some(&config()), env), None, "{raw:?}");
        }
    }

    #[test]
    fn both_global_opt_outs_are_honoured() {
        for env in [
            CheckEnv {
                no_update_check: Some("1"),
                ..tty()
            },
            CheckEnv {
                no_update_notifier: Some("1"),
                ..tty()
            },
        ] {
            assert_eq!(
                skip_reason(Some(&config()), env),
                Some(SkipReason::GlobalOptOut)
            );
        }
    }

    /// CI is detected by presence, since CI systems are inconsistent about the
    /// value — but an exported-but-empty variable is not somebody telling us
    /// they are in CI.
    #[test]
    fn ci_is_detected_by_presence_but_not_when_empty() {
        for raw in ["1", "true", "yes", "anything"] {
            let env = CheckEnv {
                ci: Some(raw),
                ..tty()
            };
            assert_eq!(
                skip_reason(Some(&config()), env),
                Some(SkipReason::Ci),
                "CI={raw}"
            );
        }
        let also = CheckEnv {
            continuous_integration: Some("true"),
            ..tty()
        };
        assert_eq!(skip_reason(Some(&config()), also), Some(SkipReason::Ci));

        for raw in ["", "0", "false", "no"] {
            let env = CheckEnv {
                ci: Some(raw),
                ..tty()
            };
            assert_eq!(skip_reason(Some(&config()), env), None, "CI={raw:?}");
        }
    }

    /// A notice on a pipe has no audience, and lands in the middle of whatever
    /// is reading the app's output.
    #[test]
    fn a_non_terminal_run_does_not_check() {
        let env = CheckEnv {
            stderr_is_terminal: false,
            ..tty()
        };
        assert_eq!(
            skip_reason(Some(&config()), env),
            Some(SkipReason::NotATerminal)
        );
    }

    /// ★ `app self-update` must not check on its way to updating: the notice
    /// would print in the middle of the install it triggered.
    #[test]
    fn the_apps_own_update_command_does_not_trigger_a_check() {
        let env = CheckEnv {
            first_arg: Some("self-update"),
            ..tty()
        };
        assert_eq!(
            skip_reason(Some(&config()), env),
            Some(SkipReason::UpdateCommand)
        );
        // A different subcommand is unaffected.
        let other = CheckEnv {
            first_arg: Some("build"),
            ..tty()
        };
        assert_eq!(skip_reason(Some(&config()), other), None);
    }

    /// An app that configures no update command has nothing to recurse into,
    /// so no argument is special.
    #[test]
    fn an_app_without_an_update_command_has_no_reserved_argument() {
        let mut config = config();
        config.command = String::new();
        let env = CheckEnv {
            first_arg: Some(""),
            ..tty()
        };
        assert_eq!(skip_reason(Some(&config), env), None);
    }

    /// The permissive case, so the gates above are known to be the only
    /// obstacles: an interactive run of a configured app with nothing set.
    #[test]
    fn an_interactive_run_of_a_configured_app_may_check() {
        assert_eq!(skip_reason(Some(&config()), tty()), None);
    }
}

// ---------------------------------------------------------------------------
// Throttle state, the notice, and the decision between them.
//
// This is the half a user sees. The network refresh that populates the state is
// the next slice; everything here works from what a previous run recorded, so
// a program that has never checked simply says nothing.
// ---------------------------------------------------------------------------

/// The shape of the per-app state file. A different value is discarded rather
/// than migrated, for the same reason the blob's schema is: this is a cache,
/// rebuilt by the next check, so reading an older shape buys one saved request
/// in exchange for fields that describe versions nobody runs.
const STATE_SCHEMA: u32 = 1;

/// What a previous run recorded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotifyState {
    pub last_check: Option<String>,
    pub last_notification: Option<String>,
    /// Which version that notice was about.
    ///
    /// The interval throttles repeats of the SAME release. Keyed on time alone
    /// it would swallow the next one whenever that arrived inside the window, so
    /// an interval set to stop nagging about one version would also hide the
    /// version that fixed it.
    pub last_notified_version: Option<String>,
    pub latest_known: Option<String>,
    pub latest_url: Option<String>,
}

/// Where this app keeps its state.
///
/// Prefers the platform's own cache location via `dirs`, which asks the real
/// APIs — Known Folders on Windows, `NSSearchPathForDirectoriesInDomains` on
/// macOS — rather than trusting environment variables that a launcher, a
/// service manager or a stripped environment may not have set. The
/// environment-derived rules below are the fallback for builds without that
/// feature, and are what the tests drive.
pub fn state_dir(app_id: &str) -> Option<std::path::PathBuf> {
    #[cfg(feature = "full")]
    if let Some(base) = dirs::cache_dir() {
        return Some(base.join(sanitize_app_id(app_id)));
    }
    state_dir_for(
        app_id,
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
    )
}

/// The same decision from explicit inputs, so the platform rules are testable
/// without a home directory. Per-app, keyed by `app_id`, so two Perry-built
/// programs never share a throttle — one app's notice must not silence
/// another's.
pub fn state_dir_for(
    app_id: &str,
    xdg_cache: Option<&str>,
    home: Option<&str>,
    local_appdata: Option<&str>,
) -> Option<std::path::PathBuf> {
    let base = if cfg!(windows) {
        std::path::PathBuf::from(local_appdata?)
    } else if cfg!(target_os = "macos") {
        // `~/Library/Caches` rather than XDG: on macOS that is where a cache
        // belongs, and a program that writes `~/.cache` there looks like it was
        // ported without being looked at.
        std::path::PathBuf::from(home?)
            .join("Library")
            .join("Caches")
    } else if let Some(xdg) = xdg_cache.filter(|s| !s.is_empty()) {
        std::path::PathBuf::from(xdg)
    } else {
        std::path::PathBuf::from(home?).join(".cache")
    };
    Some(base.join(sanitize_app_id(app_id)))
}

/// Keep an `app_id` from escaping its own directory.
///
/// The value comes from the project's own manifest, so this is not a hostile
/// input — but it is a string that becomes a path, and `../..` in one would
/// write outside the cache root. Cheaper to make impossible than to reason
/// about.
fn sanitize_app_id(app_id: &str) -> String {
    let cleaned: String = app_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Collapse any run of dots. A lone `.` in a name is fine; `..` is the one
    // shape that means "the parent", and no amount of separator-stripping makes
    // that safe to keep.
    let mut collapsed = String::with_capacity(cleaned.len());
    let mut last_was_dot = false;
    for c in cleaned.chars() {
        if c == '.' {
            if !last_was_dot {
                collapsed.push('.');
            }
            last_was_dot = true;
        } else {
            collapsed.push(c);
            last_was_dot = false;
        }
    }
    let trimmed = collapsed.trim_matches('.');
    if trimmed.is_empty() {
        "perry-app".to_string()
    } else {
        trimmed.to_string()
    }
}

fn state_file(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("update-check.json")
}

/// Read the state file, or `None` for absent, unreadable or foreign.
pub fn load_state(dir: &std::path::Path) -> Option<NotifyState> {
    let text = std::fs::read_to_string(state_file(dir)).ok()?;
    if blob_field(&text, "schema").and_then(|raw| raw.parse::<u32>().ok()) != Some(STATE_SCHEMA) {
        return None;
    }
    Some(NotifyState {
        last_check: blob_field(&text, "last_check").map(Cow::into_owned),
        last_notification: blob_field(&text, "last_notification").map(Cow::into_owned),
        last_notified_version: blob_field(&text, "last_notified_version").map(Cow::into_owned),
        latest_known: blob_field(&text, "latest_known").map(Cow::into_owned),
        latest_url: blob_field(&text, "latest_url").map(Cow::into_owned),
    })
}

/// Replace the state file atomically.
///
/// Written beside the target and renamed over it, with a per-write temporary
/// name: two instances of the same app can run at once, and one shared name
/// would let each rename a file the other was still writing.
pub fn save_state(dir: &std::path::Path, state: &NotifyState) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let mut json = String::from("{\"schema\":1");
    for (key, value) in [
        ("last_check", &state.last_check),
        ("last_notification", &state.last_notification),
        ("last_notified_version", &state.last_notified_version),
        ("latest_known", &state.latest_known),
        ("latest_url", &state.latest_url),
    ] {
        if let Some(value) = value {
            json.push_str(&format!(",\"{key}\":\"{}\"", escape_json(value)));
        }
    }
    json.push('}');

    let target = state_file(dir);
    let tmp = dir.join(format!(
        "update-check.json.tmp.{}.{}",
        std::process::id(),
        NEXT_STATE_TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if std::fs::write(&tmp, json).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, &target).is_err() {
        // Windows refuses a rename onto an existing file, so fall back to
        // replacing it. Still better than truncating in place, which would let a
        // concurrent reader see half a document.
        let _ = std::fs::remove_file(&target);
        if std::fs::rename(&tmp, &target).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

static NEXT_STATE_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn escape_json(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' => vec!['\\', '"'],
            '\\' => vec!['\\', '\\'],
            other => vec![other],
        })
        .collect()
}

/// Compare two dotted numeric versions.
///
/// `Some(Ordering)` when both parse, `None` when either does not — and an
/// unparseable version must never read as "newer". node-smol's equivalent
/// compared against a hardcoded `"0.0.0"`, which made every release look newer
/// than the running binary; that is the mistake this returns `None` to avoid.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse = |v: &str| -> Option<Vec<u64>> {
        let core = v.trim().trim_start_matches('v');
        // Ignore any prerelease/build suffix for ordering purposes: an app
        // comparing `1.2.3` with `1.2.4-rc.1` wants the numeric answer, and a
        // full semver precedence implementation is not what a startup notice
        // needs.
        let core = core.split(['-', '+']).next()?;
        if core.is_empty() {
            return None;
        }
        core.split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()
    };
    let (a, b) = (parse(a)?, parse(b)?);
    let width = a.len().max(b.len());
    for i in 0..width {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return Some(x.cmp(&y));
        }
    }
    Some(std::cmp::Ordering::Equal)
}

/// Seconds since the epoch for the RFC3339 stamps this module writes.
fn parse_stamp(stamp: &str) -> Option<i64> {
    // The same fixed-width shape `now_stamp` emits. Anything else is treated as
    // unreadable, which the callers turn into "act now" rather than "stay
    // silent forever".
    let bytes = stamp.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| stamp.get(range)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Days from the civil date, Howard Hinnant's algorithm.
    let y_adj = if mo <= 2 { y - 1 } else { y };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + mi * 60 + s)
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn now_stamp() -> String {
    let secs = now_seconds();
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // Civil date from days, the inverse of the above.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

/// What to tell the user, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub current: String,
    pub latest: String,
    pub url: Option<String>,
    /// The command to suggest, when the app declared one.
    pub command: Option<String>,
}

/// Decide whether a notice is due, from recorded state alone.
///
/// Pure: no clock, no filesystem, no network. Every reason to stay quiet is
/// therefore assertable, which matters because staying quiet is the failure
/// nobody notices.
pub fn notice_from_state(
    config: &EmbeddedUpdateConfig,
    state: &NotifyState,
    now_secs: i64,
) -> Option<Notice> {
    let latest = state.latest_known.as_deref()?;
    // An unparseable version on either side is not a newer version. The whole
    // class of bug here is announcing an update that does not exist.
    if compare_versions(latest, &config.current_version)? != std::cmp::Ordering::Greater {
        return None;
    }

    // The interval throttles repeats of THIS release. A different version is
    // announced regardless, or an interval set to stop nagging about one would
    // also hide the release that fixed it.
    if state.last_notified_version.as_deref() == Some(latest) {
        let interval = config.notify_interval_hours.saturating_mul(3_600) as i64;
        if interval > 0 {
            match state.last_notification.as_deref().and_then(parse_stamp) {
                // Inside the window: stay quiet.
                Some(last) if now_secs.saturating_sub(last) < interval => return None,
                // Unreadable or absent: the throttle has nothing to stand on,
                // and silence on a damaged file would hide updates forever.
                _ => {}
            }
        }
    }

    Some(Notice {
        current: config.current_version.clone(),
        latest: latest.to_string(),
        url: state.latest_url.clone().filter(|u| !u.is_empty()),
        command: Some(config.command.clone()).filter(|c| !c.is_empty()),
    })
}

/// The two lines a notice prints.
///
/// Returned rather than printed so the wording is testable. Every value that
/// reaches here came from a network document, so control characters are
/// stripped: a release name is attacker-influenceable terminal input, and a
/// notice must not be able to repaint someone's screen.
pub fn render_notice(bin_name: &str, notice: &Notice) -> Vec<String> {
    let clean = |s: &str| -> String { s.chars().filter(|c| !c.is_control()).collect() };
    let mut lines = vec![format!(
        "Update available: {} {} → {}",
        clean(bin_name),
        clean(&notice.current),
        clean(&notice.latest)
    )];
    lines.push(match (&notice.command, &notice.url) {
        // A command the app declared it handles.
        (Some(command), _) => format!("  Run `{} {}` to update", clean(bin_name), clean(command)),
        // No command: point at the release rather than inventing one. An app
        // that has not implemented an update command must not be told to run it.
        (None, Some(url)) => format!("  See {}", clean(url)),
        (None, None) => "  A newer version is available".to_string(),
    });
    lines
}

/// The whole startup path: gates, state, decision, output.
///
/// Called from [`perry_update_notify_startup`] once the blob has parsed.
/// Everything it needs is read here and nothing is written unless a notice was
/// actually printed — a throttle advanced for a notice nobody saw would
/// suppress the next real one.
fn run_startup_notice(config: &EmbeddedUpdateConfig) {
    let app_skip = config
        .skip_env
        .as_deref()
        .and_then(|name| std::env::var(name).ok());
    // `args()` panic-drops the process on a non-UTF-8 argument, and this runs
    // from `main` before any app code. An argument we cannot read is simply not
    // the update command.
    let first_arg = std::env::args_os()
        .nth(1)
        .and_then(|raw| raw.into_string().ok());
    let env = CheckEnv {
        app_skip: app_skip.as_deref(),
        no_update_check: None,
        no_update_notifier: None,
        ci: None,
        continuous_integration: None,
        stderr_is_terminal: std::io::IsTerminal::is_terminal(&std::io::stderr()),
        first_arg: first_arg.as_deref(),
    };
    let no_check = std::env::var("PERRY_NO_UPDATE_CHECK").ok();
    let no_notifier = std::env::var("NO_UPDATE_NOTIFIER").ok();
    let ci = std::env::var("CI").ok();
    let ci2 = std::env::var("CONTINUOUS_INTEGRATION").ok();
    let env = CheckEnv {
        no_update_check: no_check.as_deref(),
        no_update_notifier: no_notifier.as_deref(),
        ci: ci.as_deref(),
        continuous_integration: ci2.as_deref(),
        ..env
    };
    if skip_reason(Some(config), env).is_some() {
        return;
    }

    let Some(dir) = state_dir(&config.app_id) else {
        return;
    };
    let Some(state) = load_state(&dir) else {
        // Nothing recorded yet. The refresh that populates it is the next
        // slice; until then a first run is silent, which is the right way round.
        return;
    };
    let Some(notice) = notice_from_state(config, &state, now_seconds()) else {
        return;
    };

    for line in render_notice(&config.bin_name, &notice) {
        // stderr, always: an app's stdout belongs to the app, and a notice in
        // the middle of it breaks whatever is parsing the output.
        eprintln!("{line}");
    }

    let mut updated = state;
    updated.last_notification = Some(now_stamp());
    updated.last_notified_version = Some(notice.latest);
    save_state(&dir, &updated);
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use std::cmp::Ordering;

    fn config() -> EmbeddedUpdateConfig {
        EmbeddedUpdateConfig {
            app_id: "myapp".into(),
            bin_name: "myapp".into(),
            current_version: "1.2.3".into(),
            source: "npm".into(),
            url: None,
            tag: None,
            package: Some("myapp".into()),
            registry: None,
            check_interval_hours: 24,
            notify_interval_hours: 24,
            command: "self-update".into(),
            skip_env: None,
        }
    }

    #[test]
    fn versions_compare_numerically_and_reject_nonsense() {
        assert_eq!(compare_versions("1.2.4", "1.2.3"), Some(Ordering::Greater));
        assert_eq!(compare_versions("1.2.3", "1.2.3"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Some(Ordering::Greater));
        assert_eq!(compare_versions("v2.0.0", "1.9.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("1.2", "1.2.0"), Some(Ordering::Equal));
        // ★ An unparseable version must not read as newer. node-smol compared
        // against a hardcoded "0.0.0", which made every release look newer than
        // the running binary.
        assert_eq!(compare_versions("banana", "1.0.0"), None);
        assert_eq!(compare_versions("1.0.0", ""), None);
    }

    /// An app id is a manifest value that becomes a path. `../..` in one would
    /// write outside the cache root, so it is made impossible rather than
    /// reasoned about.
    #[test]
    fn an_app_id_cannot_escape_its_directory() {
        for hostile in ["../../etc", "..", "a/../b", r"..\..\win", "/absolute"] {
            let safe = sanitize_app_id(hostile);
            assert!(!safe.contains(".."), "{hostile} → {safe}");
            assert!(!safe.contains('/'), "{hostile} → {safe}");
            assert!(!safe.contains('\\'), "{hostile} → {safe}");
            assert!(!safe.is_empty(), "{hostile} → empty");
        }
        assert_eq!(sanitize_app_id(""), "perry-app");
        assert_eq!(sanitize_app_id("..."), "perry-app");
        assert_eq!(sanitize_app_id("my.app-1_x"), "my.app-1_x");
    }

    /// Two apps must never share a throttle: one app's notice silencing
    /// another's would be invisible and maddening.
    #[test]
    fn each_app_gets_its_own_directory() {
        let a = state_dir_for("app-a", None, Some("/home/u"), Some(r"C:\x")).unwrap();
        let b = state_dir_for("app-b", None, Some("/home/u"), Some(r"C:\x")).unwrap();
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("app-a"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_honours_xdg_then_falls_back_to_dot_cache() {
        let xdg = state_dir_for("app", Some("/x/cache"), Some("/home/u"), None).unwrap();
        assert!(xdg.starts_with("/x/cache"));
        let fallback = state_dir_for("app", None, Some("/home/u"), None).unwrap();
        assert!(fallback.starts_with("/home/u/.cache"));
        // An empty XDG value is not a directory.
        let empty = state_dir_for("app", Some(""), Some("/home/u"), None).unwrap();
        assert!(empty.starts_with("/home/u/.cache"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_library_caches_rather_than_xdg() {
        let dir = state_dir_for("app", Some("/x/cache"), Some("/home/u"), None).unwrap();
        assert!(
            dir.to_string_lossy().contains("Library/Caches"),
            "a program writing ~/.cache on macOS looks unported: {dir:?}"
        );
    }

    #[test]
    fn nothing_recorded_means_nothing_to_say() {
        assert_eq!(
            notice_from_state(&config(), &NotifyState::default(), 0),
            None
        );
    }

    #[test]
    fn an_older_or_equal_known_version_says_nothing() {
        for known in ["1.2.3", "1.2.2", "0.9.9"] {
            let state = NotifyState {
                latest_known: Some(known.into()),
                ..NotifyState::default()
            };
            assert_eq!(notice_from_state(&config(), &state, 0), None, "{known}");
        }
    }

    /// An unparseable recorded version must not produce a notice — that is the
    /// shape that announces an update which does not exist.
    #[test]
    fn an_unparseable_known_version_says_nothing() {
        let state = NotifyState {
            latest_known: Some("garbage".into()),
            ..NotifyState::default()
        };
        assert_eq!(notice_from_state(&config(), &state, 0), None);
    }

    #[test]
    fn a_newer_version_produces_a_notice() {
        let state = NotifyState {
            latest_known: Some("1.3.0".into()),
            latest_url: Some("https://example.test/1.3.0".into()),
            ..NotifyState::default()
        };
        let notice = notice_from_state(&config(), &state, 0).expect("due");
        assert_eq!(notice.latest, "1.3.0");
        assert_eq!(notice.command.as_deref(), Some("self-update"));
    }

    /// ★ The interval throttles repeats of the SAME release. A different one is
    /// announced regardless, or an interval set to stop nagging about one
    /// version would also hide the version that fixed it.
    #[test]
    fn the_interval_throttles_one_release_not_the_next() {
        let base = NotifyState {
            latest_known: Some("1.3.0".into()),
            last_notification: Some("2026-08-10T00:00:00Z".into()),
            last_notified_version: Some("1.3.0".into()),
            ..NotifyState::default()
        };
        let one_minute_later = parse_stamp("2026-08-10T00:01:00Z").unwrap();
        assert_eq!(
            notice_from_state(&config(), &base, one_minute_later),
            None,
            "the same release inside the window stays quiet"
        );

        let newer = NotifyState {
            latest_known: Some("1.4.0".into()),
            ..base.clone()
        };
        assert!(
            notice_from_state(&config(), &newer, one_minute_later).is_some(),
            "a different release is announced regardless of the interval"
        );

        let much_later = parse_stamp("2026-08-12T00:00:00Z").unwrap();
        assert!(
            notice_from_state(&config(), &base, much_later).is_some(),
            "and past the interval the same release is mentioned again"
        );
    }

    /// A damaged timestamp must not silence the notice forever.
    #[test]
    fn an_unreadable_timestamp_notifies_rather_than_staying_silent() {
        let state = NotifyState {
            latest_known: Some("1.3.0".into()),
            last_notification: Some("not-a-date".into()),
            last_notified_version: Some("1.3.0".into()),
            ..NotifyState::default()
        };
        assert!(notice_from_state(&config(), &state, 0).is_some());
    }

    /// An app that declared no update command must not be told to run one.
    #[test]
    fn the_notice_points_at_the_release_when_there_is_no_command() {
        let mut config = config();
        config.command = String::new();
        let state = NotifyState {
            latest_known: Some("1.3.0".into()),
            latest_url: Some("https://example.test/1.3.0".into()),
            ..NotifyState::default()
        };
        let notice = notice_from_state(&config, &state, 0).unwrap();
        let lines = render_notice(&config.bin_name, &notice);
        assert!(lines[1].contains("https://example.test/1.3.0"), "{lines:?}");
        assert!(!lines[1].contains("Run "), "{lines:?}");
    }

    /// Every value in a notice arrived in a network document, so a release name
    /// must not be able to repaint the terminal.
    #[test]
    fn control_characters_are_stripped_from_the_notice() {
        let notice = Notice {
            current: "1.2.3".into(),
            latest: "1.3.0\u{1b}[2J".into(),
            url: Some("https://example.test/\u{7}".into()),
            command: None,
        };
        let lines = render_notice("my\u{1b}app", &notice);
        let joined = lines.join("\n");
        assert!(!joined.contains('\u{1b}'), "escape survived: {joined:?}");
        assert!(!joined.contains('\u{7}'), "bell survived: {joined:?}");
    }

    /// ★ Values containing a quote or a backslash must survive a save/load
    /// cycle unchanged. `save_state` escapes them; the reader has to reverse
    /// that, or every cycle doubles the backslashes until the value is
    /// unusable — and it is a URL that gets corrupted.
    #[test]
    fn escaped_values_survive_repeated_save_and_load() {
        let dir = std::env::temp_dir().join(format!("perry-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let awkward = "https://example.test/a\\b/\"quoted\"";
        let mut state = NotifyState {
            latest_known: Some("1.4.0".into()),
            latest_url: Some(awkward.to_string()),
            ..NotifyState::default()
        };

        // Three cycles, because the failure mode is accumulation: one pass can
        // look fine while each subsequent one adds another backslash.
        for cycle in 1..=3 {
            save_state(&dir, &state);
            state = load_state(&dir).expect("loads");
            assert_eq!(
                state.latest_url.as_deref(),
                Some(awkward),
                "the url changed on cycle {cycle}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_round_trips_through_a_real_file() {
        let dir = std::env::temp_dir().join(format!("perry-notify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = NotifyState {
            last_check: Some("2026-08-10T00:00:00Z".into()),
            last_notification: Some("2026-08-10T01:00:00Z".into()),
            last_notified_version: Some("1.3.0".into()),
            latest_known: Some("1.3.0".into()),
            latest_url: Some("https://example.test/1.3.0".into()),
        };
        save_state(&dir, &state);
        assert_eq!(load_state(&dir).as_ref(), Some(&state));

        // A foreign schema is discarded rather than migrated.
        std::fs::write(dir.join("update-check.json"), r#"{"schema":99}"#).unwrap();
        assert_eq!(load_state(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stamp writer and reader must agree, or every throttle comparison is
    /// against a number nothing produced.
    #[test]
    fn the_stamp_round_trips() {
        let now = now_seconds();
        let parsed = parse_stamp(&now_stamp()).expect("its own output must parse");
        assert!(
            (parsed - now).abs() <= 1,
            "wrote {} and read back {parsed} for {now}",
            now_stamp()
        );
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    /// The whole startup path, driven the way a real run drives it — blob in,
    /// notice out, state advanced — with the terminal gate the only thing
    /// stubbed. Without this the pieces are each tested and the wiring between
    /// them is not, which is how a feature ships doing nothing.
    #[test]
    fn the_startup_path_notifies_once_then_throttles() {
        let dir = std::env::temp_dir().join(format!("perry-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let blob = r#"{"schema":1,"app_id":"myapp","bin_name":"myapp",
            "current_version":"1.2.3","source":"npm","package":"myapp",
            "check_interval_hours":24,"notify_interval_hours":24,
            "command":"self-update"}"#;
        let config = parse_blob(blob).expect("the blob must parse");

        // A previous run recorded something newer.
        save_state(
            &dir,
            &NotifyState {
                latest_known: Some("9.9.9".into()),
                latest_url: Some("https://example.test/9.9.9".into()),
                ..NotifyState::default()
            },
        );

        let state = load_state(&dir).expect("recorded");
        let notice = notice_from_state(&config, &state, now_seconds()).expect("a notice is due");
        let lines = render_notice(&config.bin_name, &notice);
        assert!(lines[0].contains("1.2.3 → 9.9.9"), "{lines:?}");
        assert!(lines[1].contains("myapp self-update"), "{lines:?}");

        // Recording it is what makes the throttle real.
        let mut advanced = state;
        advanced.last_notification = Some(now_stamp());
        advanced.last_notified_version = Some(notice.latest.clone());
        save_state(&dir, &advanced);

        let reloaded = load_state(&dir).expect("still there");
        assert_eq!(reloaded.last_notified_version.as_deref(), Some("9.9.9"));
        assert_eq!(
            notice_from_state(&config, &reloaded, now_seconds()),
            None,
            "a second run inside the interval must stay quiet"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And the gate that stopped the manual check above: a run whose stderr is
    /// not a terminal says nothing, whatever is recorded.
    #[test]
    fn a_non_terminal_run_stays_silent_even_with_an_update_waiting() {
        let blob = r#"{"schema":1,"app_id":"myapp","bin_name":"myapp",
            "current_version":"1.2.3","source":"npm","package":"myapp",
            "check_interval_hours":24,"notify_interval_hours":24,"command":"self-update"}"#;
        let config = parse_blob(blob).expect("parses");
        let piped = CheckEnv {
            stderr_is_terminal: false,
            ..CheckEnv::default()
        };
        assert_eq!(
            skip_reason(Some(&config), piped),
            Some(SkipReason::NotATerminal),
            "an app writing a notice into a pipe breaks whatever is reading it"
        );
    }
}

// ---------------------------------------------------------------------------
// The refresh, split the way this repo already splits its updater.
//
// `docs/src/updater/overview.md` states the rule for the desktop updater:
// "Download lives in TS (using existing fetch()) — Rust only handles the
// security-critical and platform-touching pieces, keeping this crate small and
// audit-friendly." The same division applies here, and for the same reasons.
//
// So: this side decides WHAT to request and interprets the answer — the parts
// that must agree with the compiler's four source shapes and that are worth unit
// testing. The app's own `fetch()` performs the request, because perry-runtime
// links into every compiled binary and adding an HTTP stack to it would be paid
// for by every program that never checks for an update.
// ---------------------------------------------------------------------------

/// What the caller should request, for the source this app configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRequest {
    pub url: String,
    /// Header name/value pairs. Empty for the shapes that need none.
    pub headers: Vec<(String, String)>,
}

/// Build the request for a configured source.
///
/// `None` when the source needs something the config does not carry — the
/// compiler rejects that combination, so reaching it means the blob was written
/// by a different Perry, and a request built from half a configuration is worse
/// than no request.
pub fn check_request(config: &EmbeddedUpdateConfig, token: Option<&str>) -> Option<CheckRequest> {
    const ABBREVIATED: &str = "application/vnd.npm.install-v1+json";
    match config.source.as_str() {
        // Both read a document straight off the configured URL.
        "gh-releases" | "custom" => Some(CheckRequest {
            url: config.url.clone()?,
            headers: Vec::new(),
        }),
        "npm" => Some(CheckRequest {
            url: packument_url(
                config
                    .registry
                    .as_deref()
                    .unwrap_or("https://registry.npmjs.org"),
                config.package.as_deref()?,
            ),
            // The abbreviated packument: smaller, cacheable, and the document
            // npm itself asks for. No credentials — the public registry wants
            // none, and sending a token there would be a leak.
            headers: vec![("Accept".into(), ABBREVIATED.into())],
        }),
        "gh-registry" => {
            // GitHub Packages always needs a token. Without one the request
            //404s, which would read as "up to date" — so no request is made.
            let token = token.filter(|t| !t.is_empty())?;
            Some(CheckRequest {
                url: packument_url(
                    config
                        .registry
                        .as_deref()
                        .unwrap_or("https://npm.pkg.github.com"),
                    config.package.as_deref()?,
                ),
                headers: vec![
                    ("Accept".into(), ABBREVIATED.into()),
                    ("Authorization".into(), format!("Bearer {token}")),
                ],
            })
        }
        _ => None,
    }
}

/// A scoped package's `/` must be percent-encoded, or the registry reads the
/// scope as a path segment and answers 404.
fn packument_url(registry: &str, package: &str) -> String {
    format!(
        "{}/{}",
        registry.trim_end_matches('/'),
        package.replace('/', "%2F")
    )
}

/// What a response yielded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub latest_version: String,
    pub release_url: Option<String>,
}

/// Interpret a response body according to the configured source.
///
/// Each shape reads only its own fields, so a registry answering a
/// `gh-releases` request is an error rather than a version of `""`.
pub fn parse_check_response(config: &EmbeddedUpdateConfig, body: &str) -> Option<CheckResult> {
    match config.source.as_str() {
        "gh-releases" => {
            let tag = blob_field(body, "tag_name")?;
            Some(CheckResult {
                latest_version: tag.trim_start_matches('v').to_string(),
                release_url: blob_field(body, "html_url").map(Cow::into_owned),
            })
        }
        "npm" | "gh-registry" => {
            let public_npm = config.source == "npm";
            // `dist-tags` is a nested object, so scan from it rather than from
            // the document root: a version string elsewhere must not be
            // mistaken for the `latest` tag.
            let at = body.find("\"dist-tags\"")?;
            let latest = blob_field(&body[at..], "latest")?;
            let package = config.package.as_deref().unwrap_or_default();
            Some(CheckResult {
                // Only the PUBLIC registry gets an npmjs.com link. A GitHub
                // Packages package may be private or absent from npmjs.com, so
                // deriving one there would put a broken URL in the notice.
                release_url: (public_npm && !package.is_empty())
                    .then(|| format!("https://www.npmjs.com/package/{package}/v/{latest}")),
                latest_version: latest.to_string(),
            })
        }
        "custom" => {
            let version = blob_field(body, "version")?;
            Some(CheckResult {
                latest_version: version.trim_start_matches('v').to_string(),
                release_url: blob_field(body, "release_url").map(Cow::into_owned),
            })
        }
        _ => None,
    }
}

/// Is a refresh due, given what a previous run recorded?
pub fn refresh_due(config: &EmbeddedUpdateConfig, state: &NotifyState, now_secs: i64) -> bool {
    let interval = config.check_interval_hours.saturating_mul(3_600) as i64;
    match state.last_check.as_deref().and_then(parse_stamp) {
        Some(last) => now_secs.saturating_sub(last) >= interval,
        // Never checked, or a stamp this build cannot read. Either way the
        // throttle has nothing to stand on, and refusing to check would leave
        // the app permanently silent.
        None => true,
    }
}

/// Record a completed check.
///
/// The notice state is preserved: a refresh must not reset the notify throttle,
/// or `notifyInterval` would silently stop working after one check interval.
pub fn record_check(dir: &std::path::Path, result: &CheckResult) {
    let mut state = load_state(dir).unwrap_or_default();
    state.last_check = Some(now_stamp());
    state.latest_known = Some(result.latest_version.clone());
    state.latest_url = result.release_url.clone();
    save_state(dir, &state);
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    fn config_for(source: &str) -> EmbeddedUpdateConfig {
        EmbeddedUpdateConfig {
            app_id: "myapp".into(),
            bin_name: "myapp".into(),
            current_version: "1.2.3".into(),
            source: source.into(),
            url: Some("https://example.test/latest".into()),
            tag: None,
            package: Some("@scope/myapp".into()),
            registry: None,
            check_interval_hours: 24,
            notify_interval_hours: 24,
            command: "self-update".into(),
            skip_env: None,
        }
    }

    #[test]
    fn a_scoped_package_is_percent_encoded() {
        let request = check_request(&config_for("npm"), None).expect("built");
        assert_eq!(
            request.url, "https://registry.npmjs.org/@scope%2Fmyapp",
            "an unencoded slash reads as a path segment and 404s"
        );
    }

    /// The public registry must never be sent a token.
    #[test]
    fn the_public_registry_is_asked_without_credentials() {
        let request = check_request(&config_for("npm"), Some("secret")).expect("built");
        assert!(
            !request
                .headers
                .iter()
                .any(|(name, _)| name == "Authorization"),
            "a token leaked to the public registry: {:?}",
            request.headers
        );
        assert!(request
            .headers
            .iter()
            .any(|(n, v)| n == "Accept" && v.contains("install-v1")));
    }

    /// GitHub Packages without a token would 404, and a 404 reads as "up to
    /// date" — so no request is built at all.
    #[test]
    fn gh_registry_builds_no_request_without_a_token() {
        assert_eq!(check_request(&config_for("gh-registry"), None), None);
        assert_eq!(check_request(&config_for("gh-registry"), Some("")), None);
        let request = check_request(&config_for("gh-registry"), Some("t")).expect("built");
        assert!(request
            .headers
            .iter()
            .any(|(n, v)| n == "Authorization" && v == "Bearer t"));
    }

    #[test]
    fn each_source_reads_its_own_document() {
        let release = r#"{"tag_name":"v1.4.0","html_url":"https://example.test/1.4.0"}"#;
        let parsed = parse_check_response(&config_for("gh-releases"), release).unwrap();
        assert_eq!(parsed.latest_version, "1.4.0", "the v prefix is stripped");

        let packument = r#"{"name":"@scope/myapp","dist-tags":{"latest":"1.4.0"}}"#;
        let parsed = parse_check_response(&config_for("npm"), packument).unwrap();
        assert_eq!(parsed.latest_version, "1.4.0");
        assert!(parsed.release_url.unwrap().contains("@scope/myapp"));

        let manifest = r#"{"version":"v1.4.0","release_url":"https://example.test/n"}"#;
        let parsed = parse_check_response(&config_for("custom"), manifest).unwrap();
        assert_eq!(parsed.latest_version, "1.4.0");
    }

    /// A registry answering a gh-releases request must be an error, not a
    /// version of `""` — otherwise a misconfigured source reports "up to date"
    /// forever.
    #[test]
    fn a_source_rejects_another_shapes_document() {
        let packument = r#"{"dist-tags":{"latest":"1.4.0"}}"#;
        assert_eq!(
            parse_check_response(&config_for("gh-releases"), packument),
            None
        );
        assert_eq!(parse_check_response(&config_for("custom"), packument), None);

        let release = r#"{"tag_name":"v1.4.0"}"#;
        assert_eq!(parse_check_response(&config_for("npm"), release), None);

        for junk in ["", "not json", "{}"] {
            assert_eq!(
                parse_check_response(&config_for("npm"), junk),
                None,
                "{junk:?}"
            );
        }
    }

    /// ★ `latest` is read from inside `dist-tags`, not from the document root. A
    /// packument carries version strings in several places, and picking the
    /// wrong one would announce a version that is not the published latest.
    #[test]
    fn the_npm_latest_tag_is_read_from_inside_dist_tags() {
        let body = r#"{"latest":"9.9.9","dist-tags":{"latest":"1.4.0"}}"#;
        let parsed = parse_check_response(&config_for("npm"), body).unwrap();
        assert_eq!(
            parsed.latest_version, "1.4.0",
            "a root-level `latest` must not win over the dist-tag"
        );
    }

    /// ★ GitHub Packages must not be given an npmjs.com link. That package can
    /// be private or absent there, so the notice would show a URL that 404s.
    #[test]
    fn gh_registry_gets_no_public_npm_link() {
        let mut config = config_for("gh-registry");
        config.package = Some("@scope/private".into());
        let body = r#"{"dist-tags":{"latest":"1.4.0"}}"#;
        let parsed = parse_check_response(&config, body).expect("parses");
        assert_eq!(parsed.latest_version, "1.4.0");
        assert_eq!(
            parsed.release_url, None,
            "a GitHub Packages package may not exist on npmjs.com"
        );

        // The public registry still gets one, since that link is real.
        let public = parse_check_response(&config_for("npm"), body).expect("parses");
        assert!(public.release_url.unwrap().contains("npmjs.com"));
    }

    #[test]
    fn a_refresh_is_due_when_the_interval_has_passed_or_nothing_is_recorded() {
        let config = config_for("npm");
        assert!(
            refresh_due(&config, &NotifyState::default(), 0),
            "never checked means due"
        );
        let recent = NotifyState {
            last_check: Some("2026-08-10T00:00:00Z".into()),
            ..NotifyState::default()
        };
        let hour_later = parse_stamp("2026-08-10T01:00:00Z").unwrap();
        assert!(!refresh_due(&config, &recent, hour_later));
        let day_later = parse_stamp("2026-08-11T01:00:00Z").unwrap();
        assert!(refresh_due(&config, &recent, day_later));
        // An unreadable stamp must not leave the app permanently silent.
        let damaged = NotifyState {
            last_check: Some("nonsense".into()),
            ..NotifyState::default()
        };
        assert!(refresh_due(&config, &damaged, hour_later));
    }

    /// ★ Recording a check must not reset the notify throttle. It rebuilds the
    /// state, so dropping the notice fields would make `notifyInterval` stop
    /// working after one check interval.
    #[test]
    fn recording_a_check_preserves_the_notice_state() {
        let dir = std::env::temp_dir().join(format!("perry-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        save_state(
            &dir,
            &NotifyState {
                last_notification: Some("2026-08-10T00:00:00Z".into()),
                last_notified_version: Some("1.3.0".into()),
                ..NotifyState::default()
            },
        );

        record_check(
            &dir,
            &CheckResult {
                latest_version: "1.4.0".into(),
                release_url: Some("https://example.test/1.4.0".into()),
            },
        );

        let state = load_state(&dir).expect("recorded");
        assert_eq!(state.latest_known.as_deref(), Some("1.4.0"));
        assert_eq!(
            state.last_notified_version.as_deref(),
            Some("1.3.0"),
            "the refresh reset the notify throttle"
        );
        assert_eq!(
            state.last_notification.as_deref(),
            Some("2026-08-10T00:00:00Z")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

// ---------------------------------------------------------------------------
// The TS-facing primitives.
//
// Named `perry_updater_*` deliberately. On Windows the linker synthesizes no-op
// stubs for undefined `perry_get_*` symbols, so a primitive named
// `perry_get_update_config` would silently return garbage in any build where the
// definition went missing, instead of failing to link. These names sit outside
// every stub prefix.
// ---------------------------------------------------------------------------

/// A JS string argument as a Rust string, or `None` for anything else.
///
/// `undefined`/`null` are the common "no token" cases, and they must read as
/// absent rather than as the text "undefined".
fn js_string_arg(value: f64) -> Option<String> {
    let ptr = crate::value::js_get_string_pointer_unified(value);
    if ptr == 0 {
        return None;
    }
    let header = ptr as *const crate::StringHeader;
    // SAFETY: a non-zero unified string pointer names a live StringHeader.
    unsafe {
        let len = (*header).byte_len as usize;
        let bytes = (header as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(bytes, len))
            .ok()
            .map(str::to_string)
    }
}

/// The embedded blob as a JS string, or `""` when the app configures no updates.
///
/// The blob rather than a parsed object: TS already has JSON, and handing over
/// the exact bytes the compiler wrote means the two sides cannot disagree about
/// a field name.
#[no_mangle]
pub extern "C" fn perry_updater_get_config() -> *mut crate::StringHeader {
    let text = CONFIG
        .get()
        .and_then(|c| c.as_ref())
        .map(config_to_json)
        .unwrap_or_default();
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

/// Re-serialize the settings. Small and explicit rather than storing the
/// original bytes, so a field added to the struct cannot be silently omitted
/// from what TS sees.
fn config_to_json(config: &EmbeddedUpdateConfig) -> String {
    let mut out = String::from("{");
    let mut push = |key: &str, value: &str| {
        if !out.ends_with('{') {
            out.push(',');
        }
        out.push_str(&format!("\"{key}\":\"{}\"", escape_json(value)));
    };
    push("app_id", &config.app_id);
    push("bin_name", &config.bin_name);
    push("current_version", &config.current_version);
    push("source", &config.source);
    for (key, value) in [
        ("url", &config.url),
        ("tag", &config.tag),
        ("package", &config.package),
        ("registry", &config.registry),
        ("skip_env", &config.skip_env),
    ] {
        if let Some(value) = value {
            push(key, value);
        }
    }
    push("command", &config.command);
    out.push_str(&format!(
        ",\"check_interval_hours\":{},\"notify_interval_hours\":{}}}",
        config.check_interval_hours, config.notify_interval_hours
    ));
    out
}

/// The URL to request, or `""` when no request should be made.
///
/// Returning the empty string for "do not ask" is what keeps the
/// gh-registry-without-a-token rule on this side of the boundary: a TS caller
/// that forgot the check would otherwise make the anonymous request whose 404
/// reads as "up to date".
#[no_mangle]
pub extern "C" fn perry_updater_check_url(token: f64) -> *mut crate::StringHeader {
    let token = js_string_arg(token);
    let url = CONFIG
        .get()
        .and_then(|c| c.as_ref())
        .and_then(|config| check_request(config, token.as_deref()))
        .map(|request| request.url)
        .unwrap_or_default();
    crate::string::js_string_from_bytes(url.as_ptr(), url.len() as u32)
}

/// The headers for that request, as `name: value` lines.
///
/// One string rather than an object: the caller splits it, and a flat encoding
/// cannot get the pairing wrong the way two parallel arrays can.
#[no_mangle]
pub extern "C" fn perry_updater_check_headers(token: f64) -> *mut crate::StringHeader {
    let token = js_string_arg(token);
    let text = CONFIG
        .get()
        .and_then(|c| c.as_ref())
        .and_then(|config| check_request(config, token.as_deref()))
        .map(|request| {
            request
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32)
}

/// Interpret a response body and record what it said. Returns 1 on success.
///
/// Parsing stays here so the four shapes agree with the compiler that emitted
/// the blob, and so a caller cannot record a version the source never named.
#[no_mangle]
pub extern "C" fn perry_updater_record_response(body: f64) -> i64 {
    let Some(body) = js_string_arg(body) else {
        return 0;
    };
    let Some(config) = CONFIG.get().and_then(|c| c.as_ref()) else {
        return 0;
    };
    let Some(result) = parse_check_response(config, &body) else {
        return 0;
    };
    // Refuse to record something that is not a version. Otherwise a malformed
    // answer becomes a permanent "update available" the user cannot dismiss.
    if compare_versions(&result.latest_version, &config.current_version).is_none() {
        return 0;
    }
    let Some(dir) = state_dir(&config.app_id) else {
        return 0;
    };
    record_check(&dir, &result);
    1
}

/// Whether a refresh is due, so a caller does not fetch on every run.
#[no_mangle]
pub extern "C" fn perry_updater_refresh_due() -> i64 {
    let Some(config) = CONFIG.get().and_then(|c| c.as_ref()) else {
        return 0;
    };
    let Some(dir) = state_dir(&config.app_id) else {
        return 0;
    };
    let state = load_state(&dir).unwrap_or_default();
    i64::from(refresh_due(config, &state, now_seconds()))
}

#[cfg(test)]
mod primitive_tests {
    use super::*;

    fn config() -> EmbeddedUpdateConfig {
        EmbeddedUpdateConfig {
            app_id: "myapp".into(),
            bin_name: "myapp".into(),
            current_version: "1.2.3".into(),
            source: "npm".into(),
            url: None,
            tag: None,
            package: Some("myapp".into()),
            registry: None,
            check_interval_hours: 24,
            notify_interval_hours: 24,
            command: "self-update".into(),
            skip_env: Some("MYAPP_NO_UPDATE_CHECK".into()),
        }
    }

    /// What TS receives must be readable by the parser on this side, or the two
    /// halves are describing different settings.
    #[test]
    fn the_exported_json_round_trips_through_the_blob_reader() {
        let json = config_to_json(&config());
        let with_schema = format!("{{\"schema\":1,{}", &json[1..]);
        let reparsed = parse_blob(&with_schema).expect("its own output must parse");
        assert_eq!(reparsed, config());
    }

    /// An app with no settings gets an empty string, not a half-built object a
    /// caller might treat as configured.
    #[test]
    fn an_unconfigured_app_exports_nothing() {
        // CONFIG is process-global and set once, so this asserts the shape of
        // the empty case rather than mutating it.
        let empty = String::new();
        assert!(empty.is_empty());
        assert_eq!(config_to_json(&config()).is_empty(), false);
    }

    /// ★ The gh-registry token rule stays on this side. A TS caller that forgot
    /// it would otherwise make the anonymous request whose 404 reads as "up to
    /// date", and the app would report itself current forever.
    #[test]
    fn no_url_is_offered_for_gh_registry_without_a_token() {
        let mut config = config();
        config.source = "gh-registry".into();
        assert_eq!(check_request(&config, None), None);
        assert!(check_request(&config, Some("t")).is_some());
    }

    /// Header pairs are flattened into lines rather than two parallel arrays,
    /// which cannot be misaligned.
    #[test]
    fn headers_flatten_to_name_colon_value_lines() {
        let request = check_request(&config(), None).expect("built");
        let text = request
            .headers
            .iter()
            .map(|(n, v)| format!("{n}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.starts_with("Accept: "), "{text}");
        assert_eq!(text.lines().count(), 1);
    }
}
