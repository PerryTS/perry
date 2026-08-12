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
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Notify => "notify",
            Self::Prompt => "prompt",
            Self::Auto => "auto",
            Self::Unknown => "notify (unrecognized value in config)",
        }
    }

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
    /// Which document to read to learn the latest version:
    /// `gh-releases`, `npm`, `gh-registry` or `custom`. Unset walks the
    /// historical ladder. See `release_source`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    /// Package name for the npm-shaped sources. Defaults to Perry's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) package: Option<String>,
    /// Registry base URL for the npm-shaped sources.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) registry: Option<String>,
    /// How long a release must have existed before `auto` will install it.
    ///
    /// Defaults to 24 hours for `auto` and 0 for every other mode, so a notice
    /// still tells you about a release immediately while an unattended install
    /// waits for it to have been seen by someone. A version published and then
    /// pulled — or published by someone who should not have been able to — is
    /// most dangerous in its first hours, and this is the cheapest place to
    /// not be the first machine to run it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) min_age_hours: Option<u64>,
    /// A version the user asked not to be told about again.
    ///
    /// Written by answering `s` at the prompt. Only this exact version is
    /// suppressed — the next one notifies normally, which is what makes it
    /// different from switching the mode off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) skip_version: Option<String>,
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

    /// The cooldown, defaulted per mode: a day for `auto`, nothing otherwise.
    fn min_age(&self, mode: UpdateMode) -> Duration {
        let hours = self.min_age_hours.unwrap_or(match mode {
            UpdateMode::Auto => 24,
            _ => 0,
        });
        Duration::from_secs(hours.saturating_mul(3600))
    }
}

/// Everything the update surface needs to know about this run, resolved once.
#[derive(Debug, Clone)]
pub(crate) struct UpdatePolicy {
    /// Already accounts for the environment, CI, and whether stderr is a
    /// terminal — so a caller never has to re-derive "should I be quiet".
    /// Use [`Self::configured_mode`] when REPORTING the setting rather than
    /// acting on it: this is the decision for one run, and a suppressed run does
    /// not mean the user configured `off`.
    pub(crate) mode: UpdateMode,
    /// What the config (or `PERRY_UPDATE_MODE`) says, before this run's
    /// suppression rules.
    ///
    /// `perry doctor` exists to answer "what is my setting?", and answering with
    /// the effective mode made `doctor --format json`, `doctor | less` and every
    /// CI run report `off` regardless of the file — the exact question asked.
    pub(crate) configured_mode: UpdateMode,
    pub(crate) check_interval: Duration,
    pub(crate) notify_interval: Duration,
    pub(crate) prompt_default: bool,
    /// A complaint about the config, to print only if this run is going to
    /// speak at all.
    ///
    /// Emitting it from `resolve` would write to stderr before the precedence
    /// rules below have been applied — so `--format json`, `CI`, a piped stderr
    /// or `--quiet` would each get a stray line in the middle of output nobody
    /// asked to be interrupted. The whole point of those rules is that this run
    /// stays silent.
    pub(crate) config_warning: Option<&'static str>,
    /// Only consulted by `auto`; see the config field.
    pub(crate) min_age: Duration,
    pub(crate) skip_version: Option<String>,
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
        // Held, not printed. Emitting here would write to stderr before the
        // precedence rules above have been applied, so a `--format json` run,
        // a CI job, a piped stderr or `--quiet` would each get a stray line in
        // the middle of output nobody asked to have interrupted.
        let config_warning = matches!(config.mode, Some(UpdateMode::Unknown)).then_some(
            "warning: unrecognized `[update] mode` in ~/.perry/config.toml; using \
             \"notify\". Valid values: off, notify, prompt, auto.",
        );

        let mode = resolve_mode(env, config.mode);
        Self {
            mode,
            // `Unknown` is PRESERVED here rather than collapsed to `Notify`.
            // Its label is "notify (unrecognized value in config)", which is
            // exactly what `doctor` should say when the config has a typo in it;
            // collapsing it reported a clean "notify" and hid the typo.
            configured_mode: config.mode.unwrap_or(UpdateMode::Notify),
            check_interval: config.check_interval(),
            notify_interval: config.notify_interval(),
            prompt_default: config.prompt_default.unwrap_or(false),
            config_warning,
            min_age: config.min_age(mode),
            skip_version: config.skip_version.clone(),
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
    last_notified_version: Option<&str>,
    latest: &str,
    now_rfc3339: &str,
) -> bool {
    if notify_interval.is_zero() {
        return true;
    }
    // The interval throttles repeats of the SAME update, which is what it has
    // always said it does. Keying it on time alone silently swallowed the next
    // release whenever it arrived inside the window — so a user who set a
    // week-long interval to stop being nagged about one version would also miss
    // the one that fixed it.
    if last_notified_version != Some(latest) {
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
    now.saturating_sub(last).max(0) as u64 >= notify_interval.as_secs()
}

/// What the update surface should do at the end of a run, given the mode and
/// what the machine will allow.
///
/// Pure, and separated from doing it, because the interesting decisions here
/// are all refusals — and a refusal that only exists inside an `if` in the
/// middle of a teardown path is a refusal nobody can test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeardownAction {
    /// Say nothing at all.
    Silent,
    /// Print the notice and stop.
    Notify,
    /// Print the notice, then ask.
    Ask,
    /// Print a line and install without asking.
    Install,
    /// Print the notice, then name the command this channel understands.
    DeferToChannel(crate::install_channel::InstallChannel),
    /// Print the notice, then say the install directory is not writable.
    NeedsElevation,
    /// Print the notice and say the release is still inside its cooldown.
    TooFresh,
}

/// Inputs to [`decide_teardown`] that come from the machine rather than from
/// the user's configuration.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TeardownEnv {
    /// Did the command the user actually asked for succeed?
    pub(crate) command_succeeded: bool,
    pub(crate) stdin_is_terminal: bool,
    pub(crate) channel: crate::install_channel::InstallChannel,
    pub(crate) install_dir_writable: bool,
    /// How long the offered release has existed, when the source said. `None`
    /// means the source does not report a publish time — the abbreviated npm
    /// packument does not — and an unknown age must NOT be treated as old
    /// enough, or the cooldown silently stops applying for exactly the users
    /// whose source is cheapest to query.
    pub(crate) release_age: Option<Duration>,
}

pub(crate) fn decide_teardown(
    mode: UpdateMode,
    min_age: Duration,
    env: TeardownEnv,
) -> TeardownAction {
    use crate::install_channel::InstallChannel;

    match mode {
        UpdateMode::Off | UpdateMode::Unknown => return TeardownAction::Silent,
        UpdateMode::Notify => return TeardownAction::Notify,
        UpdateMode::Prompt | UpdateMode::Auto => {}
    }

    // Never offer to install after the command failed. The user is looking at
    // an error; a question about upgrading is noise at the worst possible
    // moment, and an unattended install would bury the error entirely.
    if !env.command_succeeded {
        return TeardownAction::Notify;
    }

    // A managed install is not ours to replace, whichever mode asked. Say what
    // the owner understands instead — a refusal with no alternative is a dead
    // end.
    if env.channel != InstallChannel::SelfManaged {
        return TeardownAction::DeferToChannel(env.channel);
    }

    // Discovered before anything is downloaded, so the failure is a sentence
    // rather than a half-finished install.
    if !env.install_dir_writable {
        return TeardownAction::NeedsElevation;
    }

    // The cooldown. Only `auto` waits: a notice should tell you about a release
    // the moment it exists, but being the first machine to unattended-install
    // one is the risk worth declining. An unknown age counts as too fresh.
    if mode == UpdateMode::Auto && !min_age.is_zero() {
        match env.release_age {
            Some(age) if age >= min_age => {}
            _ => return TeardownAction::TooFresh,
        }
    }

    match mode {
        // A prompt needs somewhere to read the answer from. stderr already
        // being a terminal is not enough — stdin can be a pipe while stderr is
        // a tty, and reading from it would either block or take whatever the
        // pipe held as consent.
        UpdateMode::Prompt if env.stdin_is_terminal => TeardownAction::Ask,
        UpdateMode::Prompt => TeardownAction::Notify,
        UpdateMode::Auto => TeardownAction::Install,
        _ => TeardownAction::Notify,
    }
}

/// Carry out [`decide_teardown`]'s answer.
///
/// Never changes the command's exit status: an update is something that
/// happens *after* the work the user asked for, so a failure here is a warning
/// on stderr and nothing more.
/// Does the notice throttle apply to this action?
///
/// Only to the ones that say something. An install is not a notice: the user
/// asked for `auto`, and having been told about the release earlier is no reason
/// to keep running the old binary.
pub(crate) fn throttle_applies(action: &TeardownAction) -> bool {
    !matches!(action, TeardownAction::Install)
}

pub(crate) fn run_teardown_action(
    policy: &UpdatePolicy,
    status: &crate::update_checker::UpdateStatus,
    command_succeeded: bool,
    use_color: bool,
    verbose: bool,
    notice_throttled: bool,
) {
    let crate::update_checker::UpdateStatus::UpdateAvailable {
        current,
        latest,
        release_url,
    } = status
    else {
        return;
    };

    if is_suppressed_by_skip(policy, latest) {
        return;
    }

    let notice = || {
        crate::update_checker::print_update_notice(current, latest, release_url, use_color);
        if let Some(headline) = crate::update_checker::load_cache().and_then(|c| c.headline) {
            eprintln!("  {headline}");
        }
        // Only record when we actually said something, or the throttle would
        // suppress the next notice on the strength of one nobody saw.
        if !crate::install_channel::running_via_sudo() {
            // The version, so the interval throttles repeats of THIS release
            // rather than of "some release" — see `should_notify`.
            crate::update_checker::record_notification(latest);
        }
    };

    // The offered release's age, when the check source reported a publish time.
    let release_age = crate::update_checker::load_cache()
        .and_then(|c| c.published_at)
        .and_then(|stamp| crate::update_checker::parse_rfc3339(stamp.as_str()))
        .and_then(|published| {
            let now =
                crate::update_checker::parse_rfc3339(&crate::update_checker::now_rfc3339_public())?;
            Some(Duration::from_secs(
                now.saturating_sub(published).max(0) as u64
            ))
        });

    let action = decide_teardown(
        policy.mode,
        policy.min_age,
        TeardownEnv {
            command_succeeded,
            stdin_is_terminal: std::io::stdin().is_terminal(),
            channel: crate::install_channel::detect(),
            install_dir_writable: crate::install_channel::install_dir_is_writable(),
            release_age,
        },
    );

    // The throttle is applied HERE, to the resolved action, rather than around
    // the whole call. `notify_interval_hours` silences repeats of a notice, and
    // gating the call gated the install with it: in `auto` mode a notice printed
    // an hour ago stopped the new version from ever landing, which is the one
    // thing `auto` promises to do.
    if notice_throttled && throttle_applies(&action) {
        return;
    }

    match action {
        TeardownAction::Silent => {}
        TeardownAction::Notify => notice(),
        TeardownAction::DeferToChannel(channel) => {
            notice();
            if let Some(command) = channel.upgrade_command() {
                eprintln!(
                    "  This perry was installed by {}, so `perry update` would \
                     overwrite it behind that tool's back. Run `{}` instead.",
                    channel.label(),
                    command
                );
                if let Some(detail) = channel.refusal_detail() {
                    eprintln!("  ({detail})");
                }
            }
        }
        TeardownAction::TooFresh => {
            notice();
            eprintln!(
                "  Holding off: this release is newer than the {} hour cooldown \
                 for unattended installs. Run `perry update` to take it now.",
                policy.min_age.as_secs() / 3600
            );
        }
        TeardownAction::NeedsElevation => {
            notice();
            eprintln!(
                "  The install directory is not writable by this user, so the \
                 update was not attempted. Run `sudo perry update`."
            );
        }
        TeardownAction::Ask => {
            notice();
            // Three answers, not two. "No" and "never tell me about THIS one"
            // are different intentions, and without the third a user who does
            // not want one specific release has to switch the whole mode off
            // to stop being asked — which then hides the release that fixes it.
            let choice = dialoguer::Select::new()
                .with_prompt(format!("Update perry to {latest}?"))
                .items(&[
                    "Yes, update now",
                    "Not now",
                    "Skip this version and stop asking about it",
                ])
                .default(if policy.prompt_default { 0 } else { 1 })
                .interact_opt()
                .unwrap_or(None);
            match choice {
                Some(0) => install_now(use_color, verbose),
                Some(2) => remember_skipped_version(latest),
                // "Not now", or the prompt was cancelled.
                _ => eprintln!("  Not updating. `perry update --mode notify` stops the question."),
            }
        }
        TeardownAction::Install => {
            eprintln!("  Installing perry {latest}...");
            install_now(use_color, verbose);
        }
    }
}

/// Has the user asked not to hear about this exact version again?
///
/// Only this one: the next release notifies normally, which is what separates
/// "not this one" from switching the mode off.
pub(crate) fn is_suppressed_by_skip(policy: &UpdatePolicy, latest: &str) -> bool {
    policy.skip_version.as_deref() == Some(latest)
}

/// Persist "do not mention this version again".
///
/// Only this exact version: the next release notifies normally, which is what
/// makes the answer different from switching the mode off.
fn remember_skipped_version(version: &str) {
    // Refuses to write when the config could not be read: `load_config` returns
    // defaults for a damaged file as well as an absent one, and writing those
    // back would destroy the user's license key and tokens.
    match crate::commands::publish::update_config_file(|config| {
        config
            .update
            .get_or_insert_with(Default::default)
            .skip_version = Some(version.to_string());
    }) {
        Ok(()) => eprintln!("  Skipping {version}. Later releases will still be mentioned."),
        Err(error) => eprintln!("warning: could not save the skip: {error}"),
    }
}

fn install_now(use_color: bool, verbose: bool) {
    if let Err(error) =
        crate::update_checker::perform_self_update(crate::update_checker::UpdateOutput {
            verbose,
            quiet: false,
            color: use_color,
        })
    {
        // A warning, never an exit status: the command the user asked for has
        // already finished, and its result is the one that matters.
        eprintln!("warning: update failed: {error}");
    }
}

#[cfg(test)]
mod teardown_tests {
    use super::*;
    use crate::install_channel::InstallChannel;

    /// ★ The notice throttle silences notices, not installs.
    ///
    /// `notify_interval_hours` used to gate the whole teardown call, so in
    /// `auto` mode a notice printed an hour earlier stopped the update from
    /// landing at all — the one thing `auto` exists to do.
    #[test]
    fn the_notice_throttle_never_holds_back_an_install() {
        assert!(
            !throttle_applies(&TeardownAction::Install),
            "an install is not a notice and must not be throttled"
        );
        for action in [
            TeardownAction::Notify,
            TeardownAction::Ask,
            TeardownAction::DeferToChannel(crate::install_channel::InstallChannel::Homebrew),
        ] {
            assert!(
                throttle_applies(&action),
                "{action:?} speaks, so the throttle applies to it"
            );
        }
    }

    /// An interactive terminal, a successful command, an unmanaged install
    /// with a writable directory — the only shape in which anything installs.
    fn ideal() -> TeardownEnv {
        TeardownEnv {
            command_succeeded: true,
            stdin_is_terminal: true,
            channel: InstallChannel::SelfManaged,
            install_dir_writable: true,
            // Old enough for any cooldown, so these cases keep testing what
            // they were written to test.
            release_age: Some(Duration::from_secs(365 * 24 * 3600)),
        }
    }

    /// No cooldown, which is every mode's default except `auto`.
    fn no_cooldown() -> Duration {
        Duration::ZERO
    }

    #[test]
    fn off_says_nothing_and_notify_only_notifies() {
        assert_eq!(
            decide_teardown(UpdateMode::Off, no_cooldown(), ideal()),
            TeardownAction::Silent
        );
        assert_eq!(
            decide_teardown(UpdateMode::Notify, no_cooldown(), ideal()),
            TeardownAction::Notify
        );
    }

    #[test]
    fn prompt_asks_and_auto_installs_when_everything_allows_it() {
        assert_eq!(
            decide_teardown(UpdateMode::Prompt, no_cooldown(), ideal()),
            TeardownAction::Ask
        );
        assert_eq!(
            decide_teardown(UpdateMode::Auto, no_cooldown(), ideal()),
            TeardownAction::Install
        );
    }

    /// ★ After a failed command, neither mode may do anything but notify. The
    /// user is reading an error; a question about upgrading is noise, and an
    /// unattended install would bury the error under progress output.
    #[test]
    fn a_failed_command_downgrades_both_active_modes_to_a_notice() {
        let failed = TeardownEnv {
            command_succeeded: false,
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Prompt, no_cooldown(), failed),
            TeardownAction::Notify
        );
        assert_eq!(
            decide_teardown(UpdateMode::Auto, no_cooldown(), failed),
            TeardownAction::Notify
        );
    }

    /// ★ The refusal that matters most. A package manager owns its record of
    /// what is installed; overwriting the binary underneath leaves that record
    /// lying.
    #[test]
    fn a_managed_install_is_never_replaced_in_place() {
        for channel in [
            InstallChannel::Homebrew,
            InstallChannel::Npm,
            InstallChannel::Apt,
            InstallChannel::Winget,
        ] {
            let env = TeardownEnv { channel, ..ideal() };
            for mode in [UpdateMode::Prompt, UpdateMode::Auto] {
                assert_eq!(
                    decide_teardown(mode, no_cooldown(), env),
                    TeardownAction::DeferToChannel(channel),
                    "{:?} on {} must defer, not install",
                    mode,
                    channel.label()
                );
            }
        }
    }

    /// Checked before anything is downloaded, so a root-owned
    /// `/usr/local/bin` produces one sentence rather than a half-finished
    /// install. Perry never escalates on its own.
    #[test]
    fn an_unwritable_install_directory_asks_for_elevation_instead_of_trying() {
        let env = TeardownEnv {
            install_dir_writable: false,
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Auto, no_cooldown(), env),
            TeardownAction::NeedsElevation
        );
        assert_eq!(
            decide_teardown(UpdateMode::Prompt, no_cooldown(), env),
            TeardownAction::NeedsElevation
        );
    }

    /// ★ Skipping one version is not the same as switching notices off. The
    /// suppressed version goes quiet; the next one does not.
    #[test]
    fn a_skipped_version_suppresses_only_itself() {
        let policy = UpdatePolicy {
            mode: UpdateMode::Notify,
            configured_mode: UpdateMode::Notify,
            check_interval: Duration::from_secs(24 * 3600),
            notify_interval: Duration::ZERO,
            prompt_default: false,
            min_age: Duration::ZERO,
            skip_version: Some("0.5.1447".to_string()),
            config_warning: None,
        };
        assert!(
            is_suppressed_by_skip(&policy, "0.5.1447"),
            "the skipped version must go quiet"
        );
        assert!(
            !is_suppressed_by_skip(&policy, "0.5.1448"),
            "the NEXT version must still be mentioned — otherwise `skip` is \
             just `off` with extra steps, and the release that fixes the \
             skipped one stays hidden"
        );
        assert!(
            !is_suppressed_by_skip(&policy, "0.5.1446"),
            "and an unrelated version is not affected"
        );
    }

    /// ★ The release cooldown. Only `auto` waits: a notice should mention a
    /// release the moment it exists, but being the first machine in the world
    /// to unattended-install one is the risk worth declining. A version
    /// published and then pulled — or published by someone who should not have
    /// been able to — is most dangerous in its first hours.
    #[test]
    fn auto_waits_out_the_cooldown_and_the_other_modes_do_not() {
        let day = Duration::from_secs(24 * 3600);
        let fresh = TeardownEnv {
            release_age: Some(Duration::from_secs(3600)),
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Auto, day, fresh),
            TeardownAction::TooFresh,
            "an hour-old release is inside a one-day cooldown"
        );

        let aged = TeardownEnv {
            release_age: Some(Duration::from_secs(25 * 3600)),
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Auto, day, aged),
            TeardownAction::Install,
            "past the cooldown it installs"
        );

        // Notify and prompt are about telling a human, who can decide for
        // themselves, so they are never held back.
        assert_eq!(
            decide_teardown(UpdateMode::Notify, day, fresh),
            TeardownAction::Notify
        );
        assert_eq!(
            decide_teardown(UpdateMode::Prompt, day, fresh),
            TeardownAction::Ask
        );
    }

    /// ★ An UNKNOWN age counts as too fresh, not as old enough.
    ///
    /// The abbreviated npm packument carries no publish time, so treating
    /// unknown as "old enough" would silently switch the cooldown off for
    /// exactly the users whose source is cheapest to query — a protection that
    /// is present in the config and absent in effect.
    #[test]
    fn an_unknown_release_age_is_treated_as_too_fresh() {
        let day = Duration::from_secs(24 * 3600);
        let unknown = TeardownEnv {
            release_age: None,
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Auto, day, unknown),
            TeardownAction::TooFresh
        );
        // ...and with the cooldown explicitly disabled, an unknown age is no
        // longer an obstacle, so someone who does not want it is not stuck.
        assert_eq!(
            decide_teardown(UpdateMode::Auto, Duration::ZERO, unknown),
            TeardownAction::Install
        );
    }

    /// The cooldown defaults per mode: a day for `auto`, nothing for the rest.
    #[test]
    fn the_cooldown_defaults_to_a_day_for_auto_only() {
        let config = UpdateConfig::default();
        assert_eq!(
            config.min_age(UpdateMode::Auto),
            Duration::from_secs(24 * 3600)
        );
        for mode in [UpdateMode::Notify, UpdateMode::Prompt, UpdateMode::Off] {
            assert_eq!(config.min_age(mode), Duration::ZERO, "{mode:?}");
        }
        // An explicit value wins for every mode, including 0 to switch it off.
        let explicit = UpdateConfig {
            min_age_hours: Some(0),
            ..UpdateConfig::default()
        };
        assert_eq!(explicit.min_age(UpdateMode::Auto), Duration::ZERO);
    }

    /// stderr being a terminal is not enough to ask a question: stdin can be a
    /// pipe at the same time, and reading from it would either block or treat
    /// whatever the pipe held as consent.
    #[test]
    fn prompt_degrades_to_notify_when_stdin_is_not_a_terminal() {
        let env = TeardownEnv {
            stdin_is_terminal: false,
            ..ideal()
        };
        assert_eq!(
            decide_teardown(UpdateMode::Prompt, no_cooldown(), env),
            TeardownAction::Notify
        );
        assert_eq!(
            decide_teardown(UpdateMode::Auto, no_cooldown(), env),
            TeardownAction::Install,
            "auto asks nothing, so it does not need stdin"
        );
    }
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

    /// The interval alone, with the announced version held constant — which is
    /// what these cases were written to exercise.
    fn notified_before(interval: Duration, last_notification: Option<&str>, now: &str) -> bool {
        should_notify(interval, last_notification, Some("1.0.0"), "1.0.0", now)
    }

    #[test]
    fn the_notify_throttle_defaults_to_every_run() {
        assert!(notified_before(
            Duration::ZERO,
            None,
            "2026-08-10T00:00:00Z"
        ));
        assert!(notified_before(
            Duration::ZERO,
            Some("2026-08-10T00:00:00Z"),
            "2026-08-10T00:00:01Z"
        ));
    }

    #[test]
    fn the_notify_throttle_honours_its_interval() {
        let day = Duration::from_secs(24 * 3600);
        assert!(
            !notified_before(day, Some("2026-08-10T00:00:00Z"), "2026-08-10T01:00:00Z"),
            "an hour into a one-day throttle must stay quiet"
        );
        assert!(
            notified_before(day, Some("2026-08-09T00:00:00Z"), "2026-08-10T01:00:00Z"),
            "past the interval it must speak up"
        );
    }

    /// ★ The interval throttles repeats of the SAME update, which is what it
    /// always claimed. Keyed on time alone it swallowed the NEXT release
    /// whenever that landed inside the window — so a week-long interval set to
    /// stop nagging about one version would also hide the version that fixed
    /// it.
    #[test]
    fn a_different_version_is_announced_regardless_of_the_interval() {
        let week = Duration::from_secs(7 * 24 * 3600);
        // One minute into a week-long throttle: the same version stays quiet...
        assert!(!should_notify(
            week,
            Some("2026-08-10T00:00:00Z"),
            Some("1.0.0"),
            "1.0.0",
            "2026-08-10T00:01:00Z"
        ));
        // ...and a different one is announced anyway.
        assert!(should_notify(
            week,
            Some("2026-08-10T00:00:00Z"),
            Some("1.0.0"),
            "1.0.1",
            "2026-08-10T00:01:00Z"
        ));
        // Never having announced anything is also "not this version".
        assert!(should_notify(
            week,
            Some("2026-08-10T00:00:00Z"),
            None,
            "1.0.0",
            "2026-08-10T00:01:00Z"
        ));
    }

    /// An enormous configured interval must still suppress, not wrap around
    /// into announcing every run. `as i64` on a `Duration`'s seconds can go
    /// negative, and a negative interval compares as "already elapsed".
    #[test]
    fn an_enormous_interval_still_suppresses() {
        let absurd = Duration::from_secs(u64::MAX);
        assert!(
            !should_notify(
                absurd,
                Some("2026-08-10T00:00:00Z"),
                Some("1.0.0"),
                "1.0.0",
                "2026-08-10T00:01:00Z"
            ),
            "a signed conversion here would read as already-elapsed and notify"
        );
    }

    /// A cache this build cannot read must not silence the notice forever —
    /// that would turn one bad write into a permanently muted checker.
    #[test]
    fn an_unreadable_timestamp_notifies_rather_than_staying_silent() {
        let day = Duration::from_secs(24 * 3600);
        assert!(notified_before(day, None, "2026-08-10T00:00:00Z"));
        assert!(notified_before(
            day,
            Some("not-a-date"),
            "2026-08-10T00:00:00Z"
        ));
        assert!(notified_before(
            day,
            Some("2026-08-10T00:00:00Z"),
            "also-not-a-date"
        ));
    }
}
