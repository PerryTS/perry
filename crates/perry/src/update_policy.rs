//! What the update checker is allowed to do this run, and how often.
//!
//! Perry has checked for updates since long before this module existed, but the
//! only way to influence it was `PERRY_NO_UPDATE_CHECK`, which is all-or-
//! nothing. There was no way to say "check less often", "ask me before
//! installing", or "just install it" — and no way to say any of them once, in a
//! config file, instead of in every shell.
//!
//! This is that surface: an `[update]` section in `~/.perry/config.toml` with a
//! mode, plus two intervals. The default is exactly what Perry did before, so a
//! user who never opens the config sees no change.
//!
//! # Why the modes are shaped this way
//!
//! `off` and `notify` are the two behaviours that already existed. `prompt` and
//! `auto` are new, and both are deliberately harder to reach than notify:
//! replacing the binary a user is running is not something to do because a
//! default was convenient.
//!
//! # Precedence, and why the kill switch stays on top
//!
//! An environment variable always beats the config file, because the config
//! file is a preference and the environment is a decision about *this* run —
//! usually made by a script, a CI job, or someone debugging. The one rule that
//! outranks everything is `PERRY_NO_UPDATE_CHECK`: it is the documented way to
//! make Perry stop touching the network, and a config file must never be able
//! to re-enable that. `NO_UPDATE_NOTIFIER` is honoured for the same reason —
//! it is the de-facto ecosystem-wide spelling (npm's `update-notifier`, and
//! `GH_NO_UPDATE_NOTIFIER` / `DENO_NO_UPDATE_CHECK` by analogy), and someone
//! who sets it has already told every other tool what they want.

use std::io::IsTerminal;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How much of the update surface is switched on.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum UpdateMode {
    /// Never check, never say anything.
    Off,
    /// Check in the background, print one line at the end of the run when a
    /// newer version exists. Perry's behaviour before this module, and the
    /// default.
    #[default]
    Notify,
    /// Notify, then ask whether to install. Only ever on an interactive
    /// terminal, and only after a command that succeeded.
    Prompt,
    /// Install without asking, at the end of a successful run. Opt-in only.
    Auto,
    /// Anything this build does not recognise.
    ///
    /// A typo in a config file must not take the whole file down with it —
    /// `load_config` parses the file as a unit and falls back to defaults on
    /// error, so a rejected `mode` would silently discard the user's license
    /// key along with it. Unknown spellings therefore parse, and
    /// [`UpdatePolicy::resolve`] treats them as `notify` after warning once.
    #[serde(other)]
    Unknown,
}

impl UpdateMode {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "notify" => Some(Self::Notify),
            "prompt" => Some(Self::Prompt),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

/// The `[update]` section of `~/.perry/config.toml`.
///
/// Every field is optional so a partially-written section round-trips without
/// inventing values the user did not write.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UpdateConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mode: Option<UpdateMode>,
    /// Where to ask what the latest version is. Pre-dates this module.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) server: Option<String>,
    /// Hours between background checks. Default 24.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) check_interval_hours: Option<u64>,
    /// Minimum hours between two notices about the same available update.
    /// Default 0, which is "every run" — what Perry did before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notify_interval_hours: Option<u64>,
    /// What Enter means at the `prompt` mode question. Default false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_default: Option<bool>,
    /// Keys this build does not know about.
    ///
    /// Without this, a `[update]` key written by a NEWER Perry — or by hand,
    /// ahead of a feature landing — is dropped on the next save, because
    /// serde reconstructs the file from the struct. That is the same defect
    /// this module was written to fix, one level down, so the escape hatch is
    /// not optional.
    #[serde(flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub(crate) extra: toml::Table,
}

impl UpdateConfig {
    fn check_interval(&self) -> Duration {
        Duration::from_secs(self.check_interval_hours.unwrap_or(24).saturating_mul(3600))
    }

    fn notify_interval(&self) -> Duration {
        Duration::from_secs(self.notify_interval_hours.unwrap_or(0).saturating_mul(3600))
    }
}

/// Everything the update surface needs to know about this run, resolved once.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UpdatePolicy {
    /// Already accounts for the environment, CI, and whether stderr is a
    /// terminal — so a caller never has to re-derive "should I be quiet".
    pub(crate) mode: UpdateMode,
    pub(crate) check_interval: Duration,
    pub(crate) notify_interval: Duration,
    pub(crate) prompt_default: bool,
}

/// The environment inputs, gathered in one place so the decision itself is a
/// pure function that tests can drive without touching the process.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PolicyEnv<'a> {
    pub(crate) no_update_check: Option<&'a str>,
    pub(crate) no_update_notifier: Option<&'a str>,
    pub(crate) mode: Option<&'a str>,
    pub(crate) ci: Option<&'a str>,
    pub(crate) stderr_is_terminal: bool,
    /// True when the command's output is machine-readable, in which case even
    /// a stderr notice is unwelcome: it lands in the middle of whatever the
    /// caller is parsing, and the classic update-notifier bug report is
    /// exactly that.
    pub(crate) structured_output: bool,
}

fn env_is_on(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("on") | Some("yes")
    )
}

/// Present and not explicitly false.
///
/// Broader than the `"true"`/`"1"` test Perry used before, because CI systems
/// are not consistent about the value — but an EMPTY value still counts as
/// absent, matching what the ecosystem does: npm's `is-ci`, which
/// `update-notifier` is built on, tests JS truthiness, and `CI=""` is falsy
/// there. An exported-but-empty variable is not somebody telling us they are
/// in CI.
fn env_is_present(raw: Option<&str>) -> bool {
    !matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        None | Some("") | Some("0") | Some("false") | Some("off") | Some("no")
    )
}

/// Resolve the mode for this run. Pure — every input is an argument.
pub(crate) fn resolve_mode(env: PolicyEnv<'_>, configured: Option<UpdateMode>) -> UpdateMode {
    // The kill switches come first and cannot be overridden by anything,
    // including `PERRY_UPDATE_MODE=auto`. Somebody who has said "do not check"
    // must not be talked out of it by a config file or a second variable.
    if env_is_on(env.no_update_check) || env_is_present(env.no_update_notifier) {
        return UpdateMode::Off;
    }
    // CI never wants a notice, and REALLY never wants an unattended install
    // partway through a pipeline.
    if env_is_present(env.ci) {
        return UpdateMode::Off;
    }
    // Nobody is reading stderr, or something is parsing stdout. Either way
    // there is no audience for a notice and no consent available for a prompt.
    if !env.stderr_is_terminal || env.structured_output {
        return UpdateMode::Off;
    }
    if let Some(raw) = env.mode {
        // An unparseable value falls through to the config rather than
        // silently selecting something: `PERRY_UPDATE_MODE=of` should not mean
        // `off`, and it should not mean `auto` either.
        if let Some(mode) = UpdateMode::parse(raw) {
            return mode;
        }
    }
    match configured {
        Some(UpdateMode::Unknown) | None => UpdateMode::Notify,
        Some(mode) => mode,
    }
}

impl UpdatePolicy {
    /// Read the environment and the config file once, and decide.
    pub(crate) fn resolve() -> Self {
        Self::resolve_with(structured_output_selected())
    }

    pub(crate) fn resolve_with(structured_output: bool) -> Self {
        let no_update_check = std::env::var("PERRY_NO_UPDATE_CHECK").ok();
        let no_update_notifier = std::env::var("NO_UPDATE_NOTIFIER").ok();
        let mode_var = std::env::var("PERRY_UPDATE_MODE").ok();
        let ci = std::env::var("CI").ok();
        let env = PolicyEnv {
            no_update_check: no_update_check.as_deref(),
            no_update_notifier: no_update_notifier.as_deref(),
            mode: mode_var.as_deref(),
            ci: ci.as_deref(),
            stderr_is_terminal: std::io::stderr().is_terminal(),
            structured_output,
        };

        let config = crate::commands::publish::load_config()
            .update
            .unwrap_or_default();
        if matches!(config.mode, Some(UpdateMode::Unknown)) {
            // One line, once, on the way past. Loud enough to fix, quiet
            // enough not to be the thing the user remembers about the run.
            eprintln!(
                "warning: unrecognized `[update] mode` in ~/.perry/config.toml; \
                 using \"notify\". Valid values: off, notify, prompt, auto."
            );
        }

        Self {
            mode: resolve_mode(env, config.mode),
            check_interval: config.check_interval(),
            notify_interval: config.notify_interval(),
            prompt_default: config.prompt_default.unwrap_or(false),
        }
    }

    /// Is the update surface switched on at all this run?
    pub(crate) fn is_active(&self) -> bool {
        self.mode != UpdateMode::Off
    }
}

/// Whether the CLI was asked for machine-readable output.
///
/// Read straight from the raw arguments rather than from the parsed `Cli`,
/// because the policy is resolved before dispatch and this is the one input
/// that has to be right on the very first line of output.
fn structured_output_selected() -> bool {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--format=") {
            return !value.eq_ignore_ascii_case("text");
        }
        if arg == "--format" {
            return !args
                .next()
                .is_some_and(|value| value.eq_ignore_ascii_case("text"));
        }
    }
    false
}

/// Has enough time passed since the last notice about this same update?
///
/// Pure so the interval arithmetic is testable without a clock or a cache
/// file. `last_notification` is whatever the cache recorded, if anything.
pub(crate) fn should_notify(
    notify_interval: Duration,
    last_notification: Option<&str>,
    now_rfc3339: &str,
) -> bool {
    if notify_interval.is_zero() {
        return true;
    }
    let (Some(last), Some(now)) = (
        crate::update_checker::parse_rfc3339(last_notification.unwrap_or("")),
        crate::update_checker::parse_rfc3339(now_rfc3339),
    ) else {
        // Never notified, or a timestamp this build cannot read. Both mean the
        // throttle has nothing to stand on, and staying silent on a damaged
        // cache would hide updates indefinitely.
        return true;
    };
    now.saturating_sub(last) >= notify_interval.as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A terminal with nothing suppressing it — the shape every precedence
    /// case below varies one field of.
    fn tty() -> PolicyEnv<'static> {
        PolicyEnv {
            stderr_is_terminal: true,
            ..PolicyEnv::default()
        }
    }

    #[test]
    fn the_default_is_what_perry_did_before() {
        assert_eq!(resolve_mode(tty(), None), UpdateMode::Notify);
    }

    /// The kill switch outranks everything, including someone else's attempt
    /// to turn the surface up. A user who has said "do not check" is not
    /// negotiating.
    #[test]
    fn the_kill_switch_beats_every_other_input() {
        let env = PolicyEnv {
            no_update_check: Some("1"),
            mode: Some("auto"),
            ..tty()
        };
        assert_eq!(resolve_mode(env, Some(UpdateMode::Auto)), UpdateMode::Off);

        // The ecosystem-standard spelling gets the same authority.
        let env = PolicyEnv {
            no_update_notifier: Some("1"),
            mode: Some("auto"),
            ..tty()
        };
        assert_eq!(resolve_mode(env, Some(UpdateMode::Auto)), UpdateMode::Off);
    }

    /// CI systems commonly set `CI=` with no value and mean yes, so presence
    /// is the test — but an explicit `CI=false` is a person saying no.
    #[test]
    fn ci_is_detected_by_presence_but_an_empty_value_is_not_ci() {
        for raw in ["1", "true", "yes", "anything"] {
            let env = PolicyEnv {
                ci: Some(raw),
                ..tty()
            };
            assert_eq!(
                resolve_mode(env, Some(UpdateMode::Auto)),
                UpdateMode::Off,
                "CI={raw:?} must suppress the update surface"
            );
        }
        // Explicitly false, and exported-but-empty, are both "not CI" — the
        // latter because `is-ci` (what npm's update-notifier uses) tests JS
        // truthiness, where an empty string is falsy.
        for raw in ["0", "false", "off", "no", ""] {
            let env = PolicyEnv {
                ci: Some(raw),
                ..tty()
            };
            assert_eq!(
                resolve_mode(env, None),
                UpdateMode::Notify,
                "CI={raw:?} is not somebody telling us they are in CI"
            );
        }
    }

    #[test]
    fn a_non_terminal_or_structured_output_run_says_nothing() {
        let piped = PolicyEnv {
            stderr_is_terminal: false,
            ..tty()
        };
        assert_eq!(resolve_mode(piped, Some(UpdateMode::Auto)), UpdateMode::Off);

        let json = PolicyEnv {
            structured_output: true,
            ..tty()
        };
        assert_eq!(resolve_mode(json, Some(UpdateMode::Auto)), UpdateMode::Off);
    }

    #[test]
    fn the_environment_beats_the_config_file() {
        let env = PolicyEnv {
            mode: Some("off"),
            ..tty()
        };
        assert_eq!(resolve_mode(env, Some(UpdateMode::Auto)), UpdateMode::Off);

        let env = PolicyEnv {
            mode: Some("AUTO"),
            ..tty()
        };
        assert_eq!(
            resolve_mode(env, Some(UpdateMode::Notify)),
            UpdateMode::Auto,
            "the spelling is case-insensitive"
        );
    }

    /// A misspelled environment value must not select a mode by accident. It
    /// falls through to the config, which is the next most specific thing the
    /// user actually said.
    #[test]
    fn an_unparseable_environment_value_falls_through() {
        let env = PolicyEnv {
            mode: Some("of"),
            ..tty()
        };
        assert_eq!(
            resolve_mode(env, Some(UpdateMode::Prompt)),
            UpdateMode::Prompt
        );
        assert_eq!(resolve_mode(env, None), UpdateMode::Notify);
    }

    /// ★ The whole-file hazard. `load_config` parses `~/.perry/config.toml` as
    /// one document and falls back to defaults on ANY error, so a `mode` that
    /// failed to deserialize would discard the user's license key and API
    /// token along with it — and the next save would write that loss to disk.
    #[test]
    fn an_unknown_mode_does_not_take_the_rest_of_the_file_with_it() {
        #[derive(Deserialize)]
        struct Wrapper {
            license_key: String,
            update: UpdateConfig,
        }
        let parsed: Wrapper =
            toml::from_str("license_key = \"keep-me\"\n[update]\nmode = \"yolo\"\n")
                .expect("an unknown mode must not fail the parse");
        assert_eq!(parsed.license_key, "keep-me");
        assert_eq!(parsed.update.mode, Some(UpdateMode::Unknown));
        assert_eq!(
            resolve_mode(tty(), parsed.update.mode),
            UpdateMode::Notify,
            "and it must resolve to the default rather than to anything surprising"
        );
    }

    /// ★ The erasure bug, one level down. A key written by a newer Perry (or
    /// by hand, ahead of the feature) must survive a load/save round trip.
    #[test]
    fn unknown_keys_inside_the_update_section_survive_a_round_trip() {
        let config: UpdateConfig =
            toml::from_str("mode = \"notify\"\nsource = \"npm\"\nfuture_key = 1\n")
                .expect("unknown keys must parse");
        let written = toml::to_string_pretty(&config).expect("serialize");
        assert!(
            written.contains("source") && written.contains("future_key"),
            "a save dropped keys it did not recognize:\n{written}"
        );
    }

    #[test]
    fn a_partial_section_round_trips_without_inventing_values() {
        let config: UpdateConfig = toml::from_str("server = \"https://example.test\"\n").unwrap();
        let written = toml::to_string_pretty(&config).unwrap();
        assert!(written.contains("server"));
        assert!(
            !written.contains("mode"),
            "an unset field must stay unset rather than being written as a default:\n{written}"
        );
    }

    #[test]
    fn the_notify_throttle_defaults_to_every_run() {
        assert!(should_notify(Duration::ZERO, None, "2026-08-10T00:00:00Z"));
        assert!(should_notify(
            Duration::ZERO,
            Some("2026-08-10T00:00:00Z"),
            "2026-08-10T00:00:01Z"
        ));
    }

    #[test]
    fn the_notify_throttle_honours_its_interval() {
        let day = Duration::from_secs(24 * 3600);
        assert!(
            !should_notify(day, Some("2026-08-10T00:00:00Z"), "2026-08-10T01:00:00Z"),
            "an hour into a one-day throttle must stay quiet"
        );
        assert!(
            should_notify(day, Some("2026-08-09T00:00:00Z"), "2026-08-10T01:00:00Z"),
            "past the interval it must speak up"
        );
    }

    /// A cache this build cannot read must not silence the notice forever —
    /// that would turn one bad write into a permanently muted checker.
    #[test]
    fn an_unreadable_timestamp_notifies_rather_than_staying_silent() {
        let day = Duration::from_secs(24 * 3600);
        assert!(should_notify(day, None, "2026-08-10T00:00:00Z"));
        assert!(should_notify(
            day,
            Some("not-a-date"),
            "2026-08-10T00:00:00Z"
        ));
        assert!(should_notify(
            day,
            Some("2026-08-10T00:00:00Z"),
            "also-not-a-date"
        ));
    }
}
