# Cross-Backend Determinism

Perry can pick from four container runtimes at startup — Docker, Podman,
apple/container, Lima/nerdctl — and the same `ComposeSpec` should
produce **the same outcome** on each of them. This page describes how
Perry guarantees that across CLIs that diverge sharply in flag shape
and feature support.

> **TL;DR**: Each backend declares its real capabilities in a typed
> table. Specs run through a normalization pass that drops fields the
> backend can't honor (with explicit warnings) before the CLI sees
> them. A conformance test suite makes "do all backends behave the
> same?" a CI-blocking check, not a runtime surprise.

## The problem

A `ComposeSpec` written for Docker that sets `privileged: true` and
`seccomp: "/etc/seccomp.json"` is meaningless on apple/container — the
runtime has no concept of privileged mode and no syscall-filter
profiles. Pre-v0.5.374 Perry handled this in two failure modes:

- **Silent rejection** — the CLI errored with an opaque
  `unknown flag --privileged` and the user spent half an hour
  hunting through Perry's source.
- **Silent downgrade** — Perry's apple protocol simply didn't emit
  the flag, and the user got a *less secure* container than they
  asked for, with no signal that the policy wasn't honored.

Both are unacceptable for production.

## The architecture

**Four orthogonal layers**, each with a single responsibility:

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 4: Conformance test suite                            │
│  "do all backends behave the same?"  → CI-blocking          │
├─────────────────────────────────────────────────────────────┤
│  Layer 3: Spec normalization + EnforcementMode              │
│  "drop / translate / hard-reject features the backend       │
│   can't honor before they reach the CLI"                    │
├─────────────────────────────────────────────────────────────┤
│  Layer 2: BackendCapabilities (declared support, 20 axes)   │
│  Native / Emulated / Partial(reason) / Unsupported          │
├─────────────────────────────────────────────────────────────┤
│  Layer 1: Backend selection (FOUR mechanisms)               │
│  1. Auto-detect via platform priority [default]             │
│  2. PERRY_CONTAINER_BACKEND env var [process]               │
│  3. setBackend(name) [TS-runtime]                           │
│  4. selectBackendFor(spec) [capability-match]               │
└─────────────────────────────────────────────────────────────┘
```

### 0. Backend selection — four mechanisms, caller chooses

| # | Mechanism | When | API |
|---|---|---|---|
| 1 | Auto-detect | "just work" | walks platform priority list on first use |
| 2 | Env var | process-level pin | `PERRY_CONTAINER_BACKEND=docker ./app` |
| 3 | Programmatic pin | TS-runtime override before first op | `await setBackend('podman')` |
| 4 | Capability-aware | pick the best backend **for the spec** | `JSON.parse(selectBackendFor(JSON.stringify(spec)))` |

The four mechanisms compose. The most common production pattern combines (4) and (3):

```typescript
import { selectBackendFor, setBackend, up } from 'perry/container';

const best = JSON.parse(selectBackendFor(JSON.stringify(spec))) as string;
// privileged: true rules out apple/container → returns "docker"
// trivial spec on macOS → returns "apple/container"

await setBackend(best);
await up(spec);
```

**`selectBackendFor` is pure** — no probes, no daemon checks, no
filesystem access. Same `(spec, mode)` always returns the same name.
Three strictness modes:

| Mode | What counts as "supported" |
|---|---|
| `"strict-native"` | Only `Native` |
| `"accept-emulated"` (default) | `Native` + `Emulated` |
| `"accept-partial"` | `Native` + `Emulated` + `Partial(reason)` |

`StrictNative` is for production parity. `AcceptEmulated` is the
sensible default. `AcceptPartial` is for dev / "just make it run."

**Companion APIs:**

```typescript
// "What backend is currently active?"
console.log(getBackend());                                // "docker"

// "What's the platform's auto-detect probe order?" (compile-time, no probes)
console.log(JSON.parse(getBackendPriority()));            // ["apple/container", ...]

// "Which backends are installed and reachable?" (probes ALL candidates)
const all = JSON.parse(await getAvailableBackends()) as BackendInfo[];
//   length === getBackendPriority().length
//   ordered by priority
//   `available: true` on the ones that probe cleanly, `available: false`
//   + `reason` on the rest
const ready = all.filter(b => b.available);

// "Try them in order — first available wins." (mutates singleton)
await setBackends(ready.map(b => b.name));

// "What does detect_backend() return?" (asymmetric — short-circuits on
// first success and returns just the winner, or full failure list on
// no-match). Keep `getAvailableBackends()` for diagnostics; use
// `detectBackend()` when you only care about the active backend.
console.log(JSON.parse(await detectBackend()));           // BackendInfo[]
```

### 1. `BackendCapabilities` — declared support, not assumed parity

Each protocol publishes a `BackendCapabilities` constant naming its
real support per axis. Field names are stable across backends — values
diverge.

```rust
pub struct BackendCapabilities {
    pub backend: &'static str,
    pub privileged: FeatureSupport,
    pub seccomp_profile: FeatureSupport,
    pub no_new_privileges: FeatureSupport,
    pub linux_capabilities: FeatureSupport,
    pub read_only_rootfs: FeatureSupport,
    pub run_as_user: FeatureSupport,
    pub network_alias: FeatureSupport,
    pub user_defined_bridge: FeatureSupport,
    pub internal_network: FeatureSupport,
    pub ipc_namespace_share: FeatureSupport,
    pub pid_namespace_share: FeatureSupport,
    pub restart_policy: FeatureSupport,
    pub healthcheck_native: FeatureSupport,
    pub rm_on_exit: FeatureSupport,
    pub named_volumes: FeatureSupport,
    pub bind_mounts: FeatureSupport,
    pub selinux_mount_labels: FeatureSupport,
    pub tmpfs_mounts: FeatureSupport,
    pub image_signature_verify: FeatureSupport,
    pub multi_arch_pull: FeatureSupport,
}

pub enum FeatureSupport {
    Native,                    // tested + emitted as-is
    Emulated,                  // engine emulates host-side
    Unsupported,               // dropped + warning
    Partial(&'static str),     // limited subset; reason documented
}
```

The actual support matrix at v0.5.374:

| Feature | Docker | Podman | apple/container | Lima |
|---|---|---|---|---|
| `privileged` | Native | Native | **Unsupported** | Native |
| `seccomp_profile` | Native | Native | **Unsupported** | Native |
| `no_new_privileges` | Native | Native | **Unsupported** | Native |
| `linux_capabilities` | Native | Native | Native | Native |
| `read_only_rootfs` | Native | Native | Native | Native |
| `run_as_user` | Native | Native | Native | Native |
| `network_alias` | Native | Native | Native (≥0.12) | Native |
| `user_defined_bridge` | Native | Native | Partial *(needs `container system start`)* | Native |
| `internal_network` | Native | Native | **Unsupported** | Native |
| `ipc_namespace_share` | Native | Native | **Unsupported** | Native |
| `pid_namespace_share` | Native | Native | **Unsupported** | Native |
| `restart_policy` | Native | Native | **Emulated** | Partial *(only `always` / `on-failure`)* |
| `healthcheck_native` | Native | Native | **Emulated** | Native |
| `rm_on_exit` | Native | Native | Native | Native |
| `named_volumes` | Native | Native | Native | Native |
| `bind_mounts` | Native | Native | Native | Native |
| `selinux_mount_labels` | Native | Native | **Unsupported** | Native |
| `tmpfs_mounts` | Native | Native | Native | Native |
| `image_signature_verify` | Native | Native | **Emulated** | Native |
| `multi_arch_pull` | Native | Native | Native | Partial *(nerdctl <1.7 limited)* |

Each protocol returns its constant from a `capabilities()` method:

```rust
impl CliProtocol for AppleContainerProtocol {
    fn capabilities(&self) -> &'static BackendCapabilities {
        &BackendCapabilities::APPLE
    }
    // ... arg builders
}
```

### 2. Spec normalization — drop unsupported fields before emit

[`CliBackend::run_with_security`](https://github.com/perry-ts/perry/blob/main/crates/perry-container-compose/src/backend.rs)
runs the normaliser **before** the protocol's `run_args()`:

```rust
let caps = self.protocol.capabilities();
let mut normalised = spec.clone();
let warnings = normalise_spec_for(caps, name, &mut normalised);
for w in &warnings {
    tracing::warn!(
        target: "perry::container::normalise",
        backend = w.backend, service = %w.service,
        field = w.field, reason = %w.reason,
        "spec field dropped/translated for backend"
    );
}
let args = self.protocol.run_args(&normalised); // <-- clean spec
```

The normaliser is **idempotent** — calling it twice on the same spec
yields the same result. It produces a `Vec<NormalizationWarning>`:

```rust
pub struct NormalizationWarning {
    pub backend: &'static str,
    pub service: String,
    pub field: &'static str,
    pub action: NormalizationAction,
    pub reason: String,
}

pub enum NormalizationAction {
    Dropped,                                       // field removed
    Translated { from: String, to: String },       // mapped to equivalent
    EmulatedHost,                                  // engine emulates instead
}
```

### 3. Enforcement mode — pick how warnings are surfaced

```rust
pub enum EnforcementMode {
    Lenient,    // default — silent tracing::warn!
    WarnUser,   // surface to TS console.warn
    Strict,     // unsupported field → hard up() failure
}
```

Production deploys that demand cross-backend parity set `Strict`.
The user opt-in says "fail if my deploy can't be reproduced exactly
across backends." Default is `Lenient` for ergonomics.

## The conformance test suite

[`tests/conformance.rs`](https://github.com/perry-ts/perry/blob/main/crates/perry-container-compose/tests/conformance.rs)
runs the **same questions against all four protocols** (19 tests).
Three categories:

### Universals — every backend MUST emit these

```rust
#[test]
fn universal_run_emits_image() {
    for (name, proto) in all_protocols() {
        let spec = baseline_spec();
        let args = proto.run_args(&spec);
        assert!(args.iter().any(|a| a == &spec.image),
                "{name}: run_args must include image; got {:?}", args);
    }
}
```

Same shape for `name`, `ports`, `volumes`, `env`, `labels`, `network-alias`,
`remove --force`, `logs --tail N`, `inspect <id>`, `pull <ref>`. A
protocol that drops one of these is fundamentally broken.

### Capability-gated — declared support is enforced

```rust
#[test]
fn capability_apple_drops_privileged_via_normalization() {
    let mut spec = ContainerSpec {
        image: "alpine".into(),
        privileged: Some(true),
        ..Default::default()
    };
    let warnings =
        normalise_spec_for(&BackendCapabilities::APPLE, "svc", &mut spec);
    assert_eq!(spec.privileged, None);
    assert_eq!(warnings.len(), 1);
}
```

### Output normalization — same shape regardless of backend

```rust
#[test]
fn parse_list_output_returns_unified_container_info_shape() {
    // Docker shape (NDJSON line)
    let docker = DockerProtocol.parse_list_output(/* docker JSON */).unwrap();
    // Apple shape (JSON array of `configuration`-wrapped objects)
    let apple = AppleContainerProtocol.parse_list_output(/* apple JSON */).unwrap();
    // Both produce ContainerInfo with the same field semantics:
    assert_eq!(docker[0].id, apple[0].id);
    assert_eq!(docker[0].image, apple[0].image);
}
```

User code reading `info.status` sees `"running"` from any backend — not
`"Up 5 seconds"` from docker vs `"running"` from apple.

## What this guarantees

Given the same `ComposeSpec`:

- **Same names** — project-namespaced container/volume/network names are
  computed at the engine layer above protocols, so they're invariant.
- **Same DNS** — service-key cross-container resolution via
  `--network-alias` works identically on Docker / Podman / Lima /
  apple ≥ 0.12.
- **Same labels** — `perry.compose.project` + `perry.compose.spec_hash`
  on every container, so cleanup-by-project + spec-drift detection
  work uniformly.
- **Same `ContainerInfo` shape** from `inspect` / `list` — code that
  reads `info.status` or `info.image` works regardless of which backend
  emitted the JSON.
- **Best-effort security flag parity** — features that land natively
  are emitted; features the backend can't honor are either translated,
  dropped with explicit warning, or hard-failed (under
  `EnforcementMode::Strict`).

## What it does NOT solve

| Out of scope | Why | Where it's handled |
|---|---|---|
| Daemon running, plugin loaded | Operational state, not feature state | `check_available()` at probe time |
| Startup latency, I/O speed | Performance differs across runtimes | User chooses backend per workload |
| Image registry auth | Each runtime owns its own credential helper | Runtime-local; Perry doesn't bridge |

## Adding a new backend

The architecture turns "add backend X" into a contained checklist:

1. Add a new `pub struct XProtocol;` to `backend.rs`.
2. Implement `CliProtocol` for it — `run_args`, `parse_list_output`, etc.
3. Add a `BackendCapabilities::X` constant in `capabilities.rs`,
   honestly declaring which features X supports.
4. Override `capabilities()` on the protocol to return that constant.
5. Register the backend in `platform_candidates()` and `probe_candidate()`.
6. Add the protocol to `tests/conformance.rs::all_protocols()`.

The conformance suite immediately catches "I forgot to emit `--name`"
or "my `inspect_args` doesn't end with the id" — surfacing protocol
gaps as test failures rather than runtime surprises in user code.

## Further reading

- [SPEC.md §18](https://github.com/perry-ts/perry/blob/main/SPEC.md) —
  canonical specification of the determinism architecture.
- [`crates/perry-container-compose/src/capabilities.rs`](https://github.com/perry-ts/perry/blob/main/crates/perry-container-compose/src/capabilities.rs) —
  full source.
- [`crates/perry-container-compose/tests/conformance.rs`](https://github.com/perry-ts/perry/blob/main/crates/perry-container-compose/tests/conformance.rs) —
  the 19-test suite.
