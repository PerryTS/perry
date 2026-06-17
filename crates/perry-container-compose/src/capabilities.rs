//! Backend capabilities + spec normalization.
//!
//! ## Why this module exists
//!
//! Perry's `ContainerSpec` and `ComposeSpec` are *abstractions over OCI*,
//! but the four backends Perry can pick at runtime — Docker, Podman,
//! apple/container, Lima/nerdctl — diverge sharply on which features
//! they actually support. A spec written for Docker that sets
//! `privileged: true` and `seccomp: "/etc/seccomp.json"` is meaningless
//! on apple/container (no privileged mode, no seccomp profiles); silently
//! emitting those flags produces opaque CLI errors at runtime, and
//! silently dropping them produces a **less secure** container than the
//! user asked for, with no signal that the policy wasn't honored.
//!
//! The fix is a three-layer dance:
//!
//! 1. **Capabilities** — every backend declares what it actually supports
//!    in a `BackendCapabilities` struct. This is the contract: the
//!    feature names are stable across backends, but the values diverge.
//!
//! 2. **Normalization** — before the orchestrator hands a `ContainerSpec`
//!    to a `CliProtocol::run_args`, it runs `normalise_spec_for(backend,
//!    spec)`. This pass either (a) translates the feature to the
//!    backend's closest equivalent (e.g., docker `--security-opt seccomp=
//!    file` → podman `--security-opt seccomp=file` ✅; apple drop with
//!    warning), (b) emits a structured `NormalizationWarning` the
//!    orchestrator surfaces to the user, or (c) raises a hard error if
//!    the user opted into `enforcement: Strict` mode.
//!
//! 3. **Conformance test suite** — `tests/conformance.rs` runs the same
//!    arg-shape assertions against every protocol's `run_args` /
//!    `create_args` / `list_args` / etc. The "did backend N emit the
//!    same shape as backend M?" question becomes a CI-blocking unit
//!    test, not a runtime surprise.
//!
//! ## Determinism guarantees
//!
//! Given the same `ComposeSpec`, normalise-then-emit produces:
//!
//! - **Same containers, same names, same labels, same volumes/networks**
//!   on every backend. Project-namespacing (`<project>_<name>`) and
//!   service-key network aliases are computed at the engine layer above
//!   the protocol, so they're invariant.
//! - **Best-effort feature parity** for security flags: features that
//!   land natively on the target runtime are emitted; features that
//!   don't are either translated (Docker's `--read-only` ↔ apple's
//!   `--read-only`), dropped with warning, or hard-rejected.
//! - **JSON output normalization** at the parse layer: `parse_list_output`
//!   on every protocol returns the **same `ContainerInfo` struct** with
//!   the same field semantics — so user code reading `info.status` sees
//!   `"running"` from any backend, not `"Up 5 seconds"` from docker
//!   vs `"running"` from apple.
//!
//! ## What this module does NOT solve
//!
//! - **Network plugin model** — apple/container's network plugin needs
//!   `container system start`; Docker daemons need to be running. Both
//!   are operational state, not feature state, so they're caught by
//!   `check_available()` in the existing trait.
//! - **Performance characteristics** — apple/container runs in a VM,
//!   Docker on macOS runs in a VM, podman rootless runs in user-namespace.
//!   Container startup time and disk I/O speed differ; that's outside
//!   the scope of "did the spec reach the runtime intact".
//! - **Image registry auth** — each backend has its own credential helper
//!   (docker `~/.docker/config.json`, podman `~/.config/containers/auth.
//!   json`, apple's keychain integration). Auth is operational state
//!   handled by the runtime; Perry doesn't try to bridge.

use crate::types::ContainerSpec;
use std::collections::BTreeSet;

/// What a backend can do. Every protocol declares its own; the engine
/// reads this before emitting a spec to ensure the spec is honorable.
///
/// Fields are deliberately named after the user-facing TS API names —
/// not the underlying CLI flags — so a feature is "supported" or not
/// regardless of whether the backend's CLI calls it `--privileged` or
/// `--system-mode=privileged` or doesn't expose it at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCapabilities {
    /// Stable identifier — `"docker"`, `"podman"`, `"apple"`, `"lima"`.
    /// Used in error messages and the conformance test suite.
    pub backend: &'static str,

    // ---- Container security ----
    /// `privileged: true` on a `ContainerSpec`. Apple/container does NOT
    /// support this (Linux containers run inside an Apple-VM; host-
    /// privilege escalation isn't a concept).
    pub privileged: FeatureSupport,

    /// `--security-opt seccomp=<file>` — syscall filtering.
    /// Apple/container does NOT support this; Docker/Podman/nerdctl do.
    pub seccomp_profile: FeatureSupport,

    /// `--security-opt no-new-privileges`. Docker/Podman support; apple
    /// doesn't expose. Important for SUID-binary defense.
    pub no_new_privileges: FeatureSupport,

    /// `--cap-add` / `--cap-drop`. Universally supported.
    pub linux_capabilities: FeatureSupport,

    /// `--read-only`. Universally supported.
    pub read_only_rootfs: FeatureSupport,

    /// `--user <UID:GID>` / `--user nobody`. Universally supported.
    pub run_as_user: FeatureSupport,

    // ---- Networking ----
    /// `--network-alias <name>` for service-key DNS. Docker, Podman,
    /// apple/container ≥ 0.12 support; older alphas silently no-op.
    pub network_alias: FeatureSupport,

    /// User-defined bridge networks (`network create --driver bridge`).
    /// Docker/Podman support; apple/container's plugin model differs
    /// (bridge is implicit; user-defined networks have other shape).
    pub user_defined_bridge: FeatureSupport,

    /// `internal: true` — network with no host egress.
    pub internal_network: FeatureSupport,

    /// `--ipc=host` / `--ipc=container:other`. Docker/Podman support;
    /// apple's VM model means IPC namespaces aren't user-controllable.
    pub ipc_namespace_share: FeatureSupport,

    /// `--pid=host` / `--pid=container:other`. Same shape as IPC.
    pub pid_namespace_share: FeatureSupport,

    // ---- Lifecycle ----
    /// `restart: <policy>` (`always`, `unless-stopped`, `on-failure`).
    /// Docker/Podman support natively; apple/container does NOT — the
    /// engine emulates `unless-stopped` via host-side respawn loop, but
    /// the other policies are dropped with warning.
    pub restart_policy: FeatureSupport,

    /// Native healthcheck via `--healthcheck-cmd` / Containerfile HEALTHCHECK
    /// or compose-spec `healthcheck:` block. Docker/Podman support
    /// natively; apple's status surface doesn't yet integrate
    /// healthchecks. Engine falls back to host-side polling.
    pub healthcheck_native: FeatureSupport,

    /// `--rm` (remove on exit). Universally supported.
    pub rm_on_exit: FeatureSupport,

    // ---- Volume / mount ----
    /// Named volumes via `--volume <name>:<path>`. Universal.
    pub named_volumes: FeatureSupport,

    /// Bind mounts via `--volume <host>:<container>`. Universal.
    pub bind_mounts: FeatureSupport,

    /// `:Z` / `:z` SELinux mount labels. Linux-only; apple/macOS irrelevant.
    pub selinux_mount_labels: FeatureSupport,

    /// `--tmpfs <path>` for in-memory filesystem mounts.
    pub tmpfs_mounts: FeatureSupport,

    // ---- Image ----
    /// Image signature verification (cosign / sigstore). Backend-side
    /// support varies; Perry's `verification.rs` runs the check before
    /// pull regardless, so this is informational.
    pub image_signature_verify: FeatureSupport,

    /// Multi-arch image pull with explicit `--platform`. Docker/Podman/
    /// apple-container all support; nerdctl partial.
    pub multi_arch_pull: FeatureSupport,
}

/// How well a feature is supported on a given backend.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FeatureSupport {
    /// Native + tested. Spec passes through unchanged.
    Native,
    /// Engine emulates the feature host-side (e.g., apple/container has
    /// no `restart: always`; the engine polls and re-runs). Slower /
    /// less reliable than native but functional.
    Emulated,
    /// Backend has no equivalent. Spec field is **dropped with warning**.
    /// In `Strict` enforcement mode, dropping is a hard error.
    Unsupported,
    /// Backend supports the feature but with a different / stricter set
    /// of allowed values. The orchestrator surfaces the constraint;
    /// users opt into the subset.
    Partial(&'static str),
}

impl FeatureSupport {
    pub fn is_native(self) -> bool {
        matches!(self, FeatureSupport::Native)
    }
    pub fn is_unsupported(self) -> bool {
        matches!(self, FeatureSupport::Unsupported)
    }
}

impl BackendCapabilities {
    pub const DOCKER: BackendCapabilities = BackendCapabilities {
        backend: "docker",
        privileged: FeatureSupport::Native,
        seccomp_profile: FeatureSupport::Native,
        no_new_privileges: FeatureSupport::Native,
        linux_capabilities: FeatureSupport::Native,
        read_only_rootfs: FeatureSupport::Native,
        run_as_user: FeatureSupport::Native,
        network_alias: FeatureSupport::Native,
        user_defined_bridge: FeatureSupport::Native,
        internal_network: FeatureSupport::Native,
        ipc_namespace_share: FeatureSupport::Native,
        pid_namespace_share: FeatureSupport::Native,
        restart_policy: FeatureSupport::Native,
        healthcheck_native: FeatureSupport::Native,
        rm_on_exit: FeatureSupport::Native,
        named_volumes: FeatureSupport::Native,
        bind_mounts: FeatureSupport::Native,
        selinux_mount_labels: FeatureSupport::Native,
        tmpfs_mounts: FeatureSupport::Native,
        image_signature_verify: FeatureSupport::Native,
        multi_arch_pull: FeatureSupport::Native,
    };

    pub const PODMAN: BackendCapabilities = BackendCapabilities {
        backend: "podman",
        privileged: FeatureSupport::Native,
        seccomp_profile: FeatureSupport::Native,
        no_new_privileges: FeatureSupport::Native,
        linux_capabilities: FeatureSupport::Native,
        read_only_rootfs: FeatureSupport::Native,
        run_as_user: FeatureSupport::Native,
        network_alias: FeatureSupport::Native,
        user_defined_bridge: FeatureSupport::Native,
        internal_network: FeatureSupport::Native,
        ipc_namespace_share: FeatureSupport::Native,
        pid_namespace_share: FeatureSupport::Native,
        restart_policy: FeatureSupport::Native,
        healthcheck_native: FeatureSupport::Native,
        rm_on_exit: FeatureSupport::Native,
        named_volumes: FeatureSupport::Native,
        bind_mounts: FeatureSupport::Native,
        selinux_mount_labels: FeatureSupport::Native,
        tmpfs_mounts: FeatureSupport::Native,
        image_signature_verify: FeatureSupport::Native,
        multi_arch_pull: FeatureSupport::Native,
    };

    pub const APPLE: BackendCapabilities = BackendCapabilities {
        backend: "apple",
        // Apple/container 0.12 — Linux containers in an Apple-VM. The
        // VM-host model means many docker-style flags don't translate.
        privileged: FeatureSupport::Unsupported,
        seccomp_profile: FeatureSupport::Unsupported,
        no_new_privileges: FeatureSupport::Unsupported,
        linux_capabilities: FeatureSupport::Native,
        read_only_rootfs: FeatureSupport::Native,
        run_as_user: FeatureSupport::Native,
        // apple/container 0.12 has `--network <name>` but **does not**
        // have `--network-alias`. Verified via `container run --help`.
        // Pre-fix this was incorrectly declared `Native`, causing the
        // engine to emit `--network-alias <svc>` for service-key DNS
        // and crash with "Unknown option '--network-alias'".
        network_alias: FeatureSupport::Unsupported,
        // User-defined bridges require the `container-network` plugin
        // (not loaded by default; needs `container system start` AND
        // a kernel installed via `container system kernel set`). When
        // unavailable, the engine logs a warning and falls through to
        // apple's implicit default network. `Partial(...)` reflects
        // "may work; documented caveat" rather than "always works".
        user_defined_bridge: FeatureSupport::Partial(
            "needs `container system start` + network plugin loaded",
        ),
        internal_network: FeatureSupport::Unsupported,
        ipc_namespace_share: FeatureSupport::Unsupported,
        pid_namespace_share: FeatureSupport::Unsupported,
        restart_policy: FeatureSupport::Emulated,
        healthcheck_native: FeatureSupport::Emulated,
        rm_on_exit: FeatureSupport::Native,
        named_volumes: FeatureSupport::Native,
        bind_mounts: FeatureSupport::Native,
        selinux_mount_labels: FeatureSupport::Unsupported,
        tmpfs_mounts: FeatureSupport::Native,
        image_signature_verify: FeatureSupport::Emulated,
        multi_arch_pull: FeatureSupport::Native,
    };

    pub const LIMA: BackendCapabilities = BackendCapabilities {
        backend: "lima",
        // Lima runs Linux in a VM with nerdctl driving the runtime —
        // most Linux features are present, but a few flags route
        // differently through nerdctl.
        privileged: FeatureSupport::Native,
        seccomp_profile: FeatureSupport::Native,
        no_new_privileges: FeatureSupport::Native,
        linux_capabilities: FeatureSupport::Native,
        read_only_rootfs: FeatureSupport::Native,
        run_as_user: FeatureSupport::Native,
        network_alias: FeatureSupport::Native,
        user_defined_bridge: FeatureSupport::Native,
        internal_network: FeatureSupport::Native,
        ipc_namespace_share: FeatureSupport::Native,
        pid_namespace_share: FeatureSupport::Native,
        restart_policy: FeatureSupport::Partial("`always` | `on-failure` only"),
        healthcheck_native: FeatureSupport::Native,
        rm_on_exit: FeatureSupport::Native,
        named_volumes: FeatureSupport::Native,
        bind_mounts: FeatureSupport::Native,
        selinux_mount_labels: FeatureSupport::Native,
        tmpfs_mounts: FeatureSupport::Native,
        image_signature_verify: FeatureSupport::Native,
        multi_arch_pull: FeatureSupport::Partial("nerdctl pre-1.7 limited"),
    };
}

/// What the orchestrator should do when normalization needs to drop
/// or translate a spec field.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum EnforcementMode {
    /// Drop unsupported fields silently with a `tracing::warn!`. Default.
    #[default]
    Lenient,
    /// Drop unsupported fields with a structured `NormalizationWarning`
    /// the engine surfaces to the user (e.g., `console.warn(...)` from
    /// the TS side).
    WarnUser,
    /// Hard-fail `up()` if any spec field can't be honored on the
    /// detected backend. The user must either change the backend or
    /// remove the field.
    Strict,
}

/// A single normalization decision. The engine collects these and
/// emits them to the user post-up().
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizationWarning {
    pub backend: &'static str,
    pub service: String,
    pub field: &'static str,
    pub action: NormalizationAction,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizationAction {
    Dropped,
    Translated { from: String, to: String },
    EmulatedHost,
}

/// Run the normalization pass for a single container spec. Returns the
/// updated spec + any warnings produced. The caller (engine) decides
/// whether to surface, log, or hard-fail on warnings based on its
/// `EnforcementMode`.
///
/// This pass is **idempotent** — running it twice on the same spec
/// produces the same output as once. The engine can call it before
/// every `run_args` invocation without state worry.
pub fn normalise_spec_for(
    caps: &BackendCapabilities,
    service_name: &str,
    spec: &mut ContainerSpec,
) -> Vec<NormalizationWarning> {
    let mut warnings = Vec::new();

    // privileged
    if spec.privileged.unwrap_or(false) && caps.privileged.is_unsupported() {
        warnings.push(NormalizationWarning {
            backend: caps.backend,
            service: service_name.into(),
            field: "privileged",
            action: NormalizationAction::Dropped,
            reason: format!(
                "backend {} does not support `privileged` mode; field dropped",
                caps.backend
            ),
        });
        spec.privileged = None;
    }

    // network_aliases — apple/container 0.12 doesn't have
    // `--network-alias`, so the engine emitting it crashes the run with
    // "Unknown option". Drop the field on backends that don't support
    // it. The user loses service-key cross-container DNS but the
    // container itself starts; sibling services that need addressing
    // can still use `container_name` pinning.
    if spec
        .network_aliases
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        && caps.network_alias.is_unsupported()
    {
        warnings.push(NormalizationWarning {
            backend: caps.backend,
            service: service_name.into(),
            field: "network_aliases",
            action: NormalizationAction::Dropped,
            reason: format!(
                "backend {} does not support `--network-alias`; \
                 service-key DNS aliases dropped — sibling services \
                 must address this container by `container_name`",
                caps.backend
            ),
        });
        spec.network_aliases = None;
    }

    // cap_add / cap_drop pruning when capabilities aren't supported is a
    // no-op today (every backend supports them); leave the field intact
    // so future audit can pin it.

    // The compose-engine layer handles seccomp via SecurityProfile, not
    // ContainerSpec, so seccomp normalization happens in
    // `normalise_security_profile` below.

    warnings
}

/// Same shape, but for `SecurityProfile` (orthogonal to `ContainerSpec`).
pub fn normalise_security_profile(
    caps: &BackendCapabilities,
    service_name: &str,
    profile: &mut crate::backend::SecurityProfile,
) -> Vec<NormalizationWarning> {
    let mut warnings = Vec::new();
    if profile.seccomp.is_some() && caps.seccomp_profile.is_unsupported() {
        warnings.push(NormalizationWarning {
            backend: caps.backend,
            service: service_name.into(),
            field: "seccomp",
            action: NormalizationAction::Dropped,
            reason: format!(
                "backend {} does not honor seccomp profiles; field dropped",
                caps.backend
            ),
        });
        profile.seccomp = None;
    }
    warnings
}

/// Inspection helper: returns the set of feature names that are not
/// natively supported on the given backend. Useful for the
/// pre-orchestration "are we going to surprise the user?" diagnostic.
pub fn unsupported_feature_names(caps: &BackendCapabilities) -> BTreeSet<&'static str> {
    let mut s = BTreeSet::new();
    macro_rules! check {
        ($field:ident) => {
            if caps.$field.is_unsupported() {
                s.insert(stringify!($field));
            }
        };
    }
    check!(privileged);
    check!(seccomp_profile);
    check!(no_new_privileges);
    check!(linux_capabilities);
    check!(read_only_rootfs);
    check!(run_as_user);
    check!(network_alias);
    check!(user_defined_bridge);
    check!(internal_network);
    check!(ipc_namespace_share);
    check!(pid_namespace_share);
    check!(restart_policy);
    check!(healthcheck_native);
    check!(rm_on_exit);
    check!(named_volumes);
    check!(bind_mounts);
    check!(selinux_mount_labels);
    check!(tmpfs_mounts);
    check!(image_signature_verify);
    check!(multi_arch_pull);
    s
}

/// Lookup the canonical `BackendCapabilities` constant for a backend name.
///
/// Names match the values returned by `platform_candidates()`. Unknown
/// names fall back to `DOCKER` (the "everything supported" baseline) so
/// any future-named OCI runtime gets reasonable defaults until its
/// capability table is wired in explicitly.
pub fn capabilities_for_backend(name: &str) -> &'static BackendCapabilities {
    match name {
        "apple/container" => &BackendCapabilities::APPLE,
        "lima" => &BackendCapabilities::LIMA,
        "podman" => &BackendCapabilities::PODMAN,
        // orbstack, colima, rancher-desktop, nerdctl, docker — all
        // Docker-protocol-compatible (orbstack + colima + rancher-desktop
        // shell out via the docker CLI; nerdctl is API-compatible). They
        // share the Docker capability profile.
        _ => &BackendCapabilities::DOCKER,
    }
}

/// Map `ComposeSpec` field usage to capability axes the backend must
/// support. Returns the minimal set of feature names a backend needs to
/// declare as `Native` (or `Emulated` / `Partial` if the caller's
/// `SelectMode` admits them) to honor this spec.
///
/// Walking each axis once with a matching field check is intentional —
/// the function is the explicit "what does the user's spec actually
/// use?" enumeration. Adding a new capability axis means: add the
/// constant in `BackendCapabilities`, then add the matching detection
/// here. The conformance test pin makes the gap loud.
pub fn required_features(
    spec: &crate::types::ComposeSpec,
) -> std::collections::BTreeSet<&'static str> {
    use std::collections::BTreeSet;
    let mut needed: BTreeSet<&'static str> = BTreeSet::new();

    for (_svc_name, svc) in &spec.services {
        // privileged: true → privileged
        if svc.privileged.unwrap_or(false) {
            needed.insert("privileged");
        }

        // security_opt seccomp=<path> → seccomp_profile
        // security_opt no-new-privileges → no_new_privileges
        if let Some(opts) = &svc.security_opt {
            for opt in opts {
                if opt.starts_with("seccomp=") || opt.starts_with("seccomp:") {
                    needed.insert("seccomp_profile");
                }
                if opt == "no-new-privileges:true"
                    || opt == "no-new-privileges=true"
                    || opt == "no-new-privileges"
                {
                    needed.insert("no_new_privileges");
                }
            }
        }

        // cap_add / cap_drop → linux_capabilities
        if svc.cap_add.as_ref().map(|v| !v.is_empty()).unwrap_or(false)
            || svc
                .cap_drop
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
        {
            needed.insert("linux_capabilities");
        }

        // read_only: true → read_only_rootfs
        if svc.read_only.unwrap_or(false) {
            needed.insert("read_only_rootfs");
        }

        // user → run_as_user
        if svc.user.is_some() {
            needed.insert("run_as_user");
        }

        // restart != "no" → restart_policy
        if let Some(restart) = &svc.restart {
            if restart != "no" {
                needed.insert("restart_policy");
            }
        }

        // healthcheck block → healthcheck_native
        if svc.healthcheck.is_some() {
            needed.insert("healthcheck_native");
        }

        // network_mode "host" / "container:..." → ipc/pid namespace sharing
        // (these flow through to docker --ipc / --pid in real specs;
        // network_mode itself is in the namespace-share family)
        // pid: "host" / "container:..." → pid_namespace_share
        if let Some(pid) = &svc.pid {
            if !pid.is_empty() && pid != "private" {
                needed.insert("pid_namespace_share");
            }
        }
        // ipc: handled via security_opt in some specs; covered above

        // tmpfs → tmpfs_mounts
        if svc.tmpfs.is_some() {
            needed.insert("tmpfs_mounts");
        }

        // volumes with :Z or :z suffix → selinux_mount_labels
        if let Some(volumes) = &svc.volumes {
            for v in volumes {
                if let Some(s) = v.as_str() {
                    if s.ends_with(":Z") || s.ends_with(":z") {
                        needed.insert("selinux_mount_labels");
                    }
                }
            }
        }
    }

    // Networks: internal: true → internal_network. The compose spec
    // allows `networks: { mynet: }` (declare with defaults) which
    // parses to `Some(name) -> None`; only check the populated case.
    if let Some(networks) = &spec.networks {
        for (_name, net_opt) in networks {
            if let Some(net) = net_opt {
                if net.internal.unwrap_or(false) {
                    needed.insert("internal_network");
                }
            }
        }
    }

    // Implicit features always needed (universal but worth declaring):
    //   network_alias — engine emits it for service-key DNS
    //   bind_mounts + named_volumes — common path
    //   rm_on_exit — when any service has `rm: true`
    // These are universal across all real backends so they don't
    // narrow selection; we omit them from `needed` to avoid noise.

    needed
}

/// How strict capability-match should be when choosing a backend.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum SelectMode {
    /// Only `Native` support counts. Any required feature with
    /// `Emulated`, `Partial`, or `Unsupported` disqualifies a backend.
    /// Use this for production deploys that demand bit-for-bit parity
    /// across runtimes (no host-side emulation surprises).
    StrictNative,
    /// `Native` + `Emulated` count; `Partial` + `Unsupported` don't.
    /// Engine-emulated features (apple's restart-loop, healthcheck
    /// polling, sigstore verification) are accepted as a degraded but
    /// functional substitute.
    #[default]
    AcceptEmulated,
    /// `Native` + `Emulated` + `Partial` count; only `Unsupported`
    /// disqualifies. Use this for development / "just make it run"
    /// flows where the partial-support reasons (e.g. apple's
    /// user-defined-bridge needs `container system start`) are
    /// acceptable.
    AcceptPartial,
}

/// Pick the highest-priority backend whose `BackendCapabilities` can
/// honor every feature the spec uses, given the strictness mode.
///
/// Walks `platform_candidates()` in priority order, looks up each
/// backend's capability table, returns the first one that satisfies
/// the spec's feature set. Returns `None` if no backend can honor the
/// spec under the given mode (Strict-mode equivalent — the caller
/// chooses whether that's an error or a fall-through to default).
///
/// The returned name can be passed to `js_container_setBackend()` or
/// `PERRY_CONTAINER_BACKEND=<name>` to pin the chosen runtime.
///
/// **Determinism:** the function is pure — same `(spec, mode)` always
/// returns the same backend name. No filesystem / network probes happen
/// here; the caller still has to verify the chosen backend is actually
/// installed via `setBackend()` (which probes) or `detect_backend()`.
pub fn select_backend_for(
    spec: &crate::types::ComposeSpec,
    mode: SelectMode,
) -> Option<&'static str> {
    let needed = required_features(spec);

    // The empty case: a trivial spec with nothing fancy → return the
    // first platform candidate (apple-first on macOS).
    if needed.is_empty() {
        return crate::backend::platform_candidates().first().copied();
    }

    for &candidate in crate::backend::platform_candidates() {
        let caps = capabilities_for_backend(candidate);
        if needed
            .iter()
            .all(|feat| feature_satisfies(caps, feat, mode))
        {
            return Some(candidate);
        }
    }
    None
}

/// Helper: given a feature axis name, look up its `FeatureSupport` on
/// the backend's capability table and decide whether the chosen
/// `SelectMode` accepts it.
fn feature_satisfies(caps: &BackendCapabilities, feature: &str, mode: SelectMode) -> bool {
    let support = match feature {
        "privileged" => caps.privileged,
        "seccomp_profile" => caps.seccomp_profile,
        "no_new_privileges" => caps.no_new_privileges,
        "linux_capabilities" => caps.linux_capabilities,
        "read_only_rootfs" => caps.read_only_rootfs,
        "run_as_user" => caps.run_as_user,
        "network_alias" => caps.network_alias,
        "user_defined_bridge" => caps.user_defined_bridge,
        "internal_network" => caps.internal_network,
        "ipc_namespace_share" => caps.ipc_namespace_share,
        "pid_namespace_share" => caps.pid_namespace_share,
        "restart_policy" => caps.restart_policy,
        "healthcheck_native" => caps.healthcheck_native,
        "rm_on_exit" => caps.rm_on_exit,
        "named_volumes" => caps.named_volumes,
        "bind_mounts" => caps.bind_mounts,
        "selinux_mount_labels" => caps.selinux_mount_labels,
        "tmpfs_mounts" => caps.tmpfs_mounts,
        "image_signature_verify" => caps.image_signature_verify,
        "multi_arch_pull" => caps.multi_arch_pull,
        // Unknown feature name — defensive: assume the backend can
        // handle it (don't block selection on a typo).
        _ => return true,
    };

    match (support, mode) {
        // Native always satisfies, regardless of mode.
        (FeatureSupport::Native, _) => true,
        // Emulated counts in AcceptEmulated + AcceptPartial.
        (FeatureSupport::Emulated, SelectMode::AcceptEmulated) => true,
        (FeatureSupport::Emulated, SelectMode::AcceptPartial) => true,
        // Partial only counts in AcceptPartial.
        (FeatureSupport::Partial(_), SelectMode::AcceptPartial) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::SecurityProfile;

    #[test]
    fn docker_supports_everything_we_care_about() {
        // Docker is the canonical "everything supported" baseline.
        // Future capability additions: keep this test as the canary —
        // any new field should default to `Native` on docker, then
        // each other backend is reasoned about explicitly.
        let unsupported = unsupported_feature_names(&BackendCapabilities::DOCKER);
        assert!(
            unsupported.is_empty(),
            "Docker should have no unsupported features; got {:?}",
            unsupported
        );
    }

    #[test]
    fn apple_unsupported_features_match_cli_reality() {
        // This test is the contract: it pins exactly which features
        // apple/container 0.12 doesn't natively support. If a future
        // apple release adds support, flip the field and update this
        // test — that's the signal to the rest of the orchestrator.
        let unsupported = unsupported_feature_names(&BackendCapabilities::APPLE);
        let expected: BTreeSet<&str> = [
            "privileged",
            "seccomp_profile",
            "no_new_privileges",
            "internal_network",
            "ipc_namespace_share",
            "pid_namespace_share",
            "selinux_mount_labels",
            // apple/container 0.12 has `--network` but NOT
            // `--network-alias`. Verified via `container run --help`
            // + the redis-smoke example crashing pre-fix with
            // "Unknown option '--network-alias'". This was Native
            // before v0.5.380; corrected to Unsupported after the
            // example-driven audit.
            "network_alias",
        ]
        .into_iter()
        .collect();
        assert_eq!(
            unsupported, expected,
            "apple/container's unsupported feature set drifted from the \
             documented capabilities; review BackendCapabilities::APPLE \
             vs `container --help` output and update the constant"
        );
    }

    #[test]
    fn normalise_drops_privileged_on_apple() {
        let mut spec = ContainerSpec {
            image: "alpine".into(),
            privileged: Some(true),
            ..Default::default()
        };
        let warnings = normalise_spec_for(&BackendCapabilities::APPLE, "svc", &mut spec);
        assert_eq!(spec.privileged, None);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "privileged");
        assert_eq!(warnings[0].backend, "apple");
        assert!(matches!(warnings[0].action, NormalizationAction::Dropped));
    }

    #[test]
    fn normalise_keeps_privileged_on_docker() {
        let mut spec = ContainerSpec {
            image: "alpine".into(),
            privileged: Some(true),
            ..Default::default()
        };
        let warnings = normalise_spec_for(&BackendCapabilities::DOCKER, "svc", &mut spec);
        assert_eq!(spec.privileged, Some(true));
        assert!(warnings.is_empty());
    }

    #[test]
    fn normalise_drops_seccomp_on_apple() {
        let mut profile = SecurityProfile {
            read_only_root: true,
            seccomp: Some("/etc/seccomp.json".into()),
            ..Default::default()
        };
        let warnings = normalise_security_profile(&BackendCapabilities::APPLE, "svc", &mut profile);
        assert_eq!(profile.seccomp, None);
        // read_only is preserved
        assert!(profile.read_only_root);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "seccomp");
    }

    #[test]
    fn normalise_keeps_seccomp_on_docker() {
        let mut profile = SecurityProfile {
            read_only_root: false,
            seccomp: Some("/etc/seccomp.json".into()),
            ..Default::default()
        };
        let warnings =
            normalise_security_profile(&BackendCapabilities::DOCKER, "svc", &mut profile);
        assert_eq!(profile.seccomp, Some("/etc/seccomp.json".into()));
        assert!(warnings.is_empty());
    }

    #[test]
    fn normalise_idempotent_on_apple() {
        let mut spec = ContainerSpec {
            image: "alpine".into(),
            privileged: Some(true),
            ..Default::default()
        };
        let _ = normalise_spec_for(&BackendCapabilities::APPLE, "svc", &mut spec);
        let warnings_pass2 = normalise_spec_for(&BackendCapabilities::APPLE, "svc", &mut spec);
        // Second call has no remaining work — spec is already clean.
        assert!(warnings_pass2.is_empty());
    }

    #[test]
    fn enforcement_mode_default_is_lenient() {
        assert_eq!(EnforcementMode::default(), EnforcementMode::Lenient);
    }

    #[test]
    fn capability_constants_have_distinct_backend_ids() {
        let names = [
            BackendCapabilities::DOCKER.backend,
            BackendCapabilities::PODMAN.backend,
            BackendCapabilities::APPLE.backend,
            BackendCapabilities::LIMA.backend,
        ];
        let unique: BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "duplicate backend identifiers");
    }
}
