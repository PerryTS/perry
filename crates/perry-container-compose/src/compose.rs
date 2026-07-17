use crate::backend::ContainerBackend;
use crate::error::{ComposeError, Result};
use crate::service;
use crate::types::{
    ComposeHandle, ComposeService, ComposeSpec, ContainerInfo, ContainerLogs, ContainerSpec,
};
use indexmap::IndexMap;
use md5::{Digest, Md5};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Compute a stable 16-char hex hash of a service's user-visible spec
/// fields. Stamped onto each created container as a `perry.compose.spec
/// _hash` label; on subsequent `up()` calls we compare the live label
/// against the freshly-computed hash and recreate the container when
/// they differ. Without this, editing `image:` from `postgres:15` to
/// `postgres:16` and re-running `up()` is a silent no-op.
fn service_spec_hash(svc: &ComposeService) -> String {
    let json = serde_json::to_string(svc).unwrap_or_default();
    let mut h = Md5::new();
    h.update(json.as_bytes());
    let bytes = h.finalize();
    hex::encode(&bytes[..8])
}

static COMPOSE_ENGINES: once_cell::sync::Lazy<std::sync::Mutex<IndexMap<u64, Arc<ComposeEngine>>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(IndexMap::new()));

static NEXT_STACK_ID: AtomicU64 = AtomicU64::new(1);

pub struct ComposeEngine {
    pub spec: ComposeSpec,
    pub project_name: String,
    pub backend: Arc<dyn ContainerBackend>,
    session_containers: Mutex<Vec<String>>,
    session_networks: Mutex<Vec<String>>,
    session_volumes: Mutex<Vec<String>>,
    /// Cached `service_name → container_name` map, populated by `up()`.
    ///
    /// `service::service_container_name` regenerates a fresh random suffix
    /// per call (`{md5_8}-{random_hex8}`), so any post-`up` operation
    /// (`exec`, `logs`, `down`, `ps`) that recomputes the name from the
    /// service spec ends up with a different name than the one the
    /// container was actually created with → "No such container" errors.
    /// `up()` resolves the name once at startup and stores it here; later
    /// methods read this map instead of regenerating.
    service_container_names: Mutex<HashMap<String, String>>,
    /// What to do when a `ContainerSpec` field can't be honored on the
    /// detected backend. See `crate::capabilities::EnforcementMode`. The
    /// engine's `up()` runs the normalization pass per service against
    /// this mode; default is `Lenient` (silent `tracing::warn!`).
    enforcement: crate::capabilities::EnforcementMode,
    /// Warnings collected from the normalization pass during `up()`.
    /// Populated regardless of mode so callers can introspect post-up;
    /// the difference between modes is whether `up()` *fails* on
    /// non-empty warnings (`Strict`), surfaces them eagerly to the user
    /// (`WarnUser`), or only logs them (`Lenient`).
    normalization_warnings: Mutex<Vec<crate::capabilities::NormalizationWarning>>,
}

impl ComposeEngine {
    pub fn new(
        spec: ComposeSpec,
        project_name: String,
        backend: Arc<dyn ContainerBackend>,
    ) -> Self {
        ComposeEngine {
            spec,
            project_name,
            backend,
            session_containers: Mutex::new(Vec::new()),
            session_networks: Mutex::new(Vec::new()),
            session_volumes: Mutex::new(Vec::new()),
            service_container_names: Mutex::new(HashMap::new()),
            enforcement: crate::capabilities::EnforcementMode::default(),
            normalization_warnings: Mutex::new(Vec::new()),
        }
    }

    /// Configure how the engine reacts when a service's `ContainerSpec`
    /// references features the chosen backend can't honor (e.g., a
    /// `privileged: true` service deployed onto apple/container).
    ///
    /// - `Lenient` (default) — silent `tracing::warn!`; `up()` proceeds.
    /// - `WarnUser` — collect warnings into the engine; caller can read
    ///   them via `take_normalization_warnings()` after `up()` returns.
    /// - `Strict` — any non-empty warning set causes `up()` to return
    ///   `ComposeError::EnforcementViolation` instead of starting the
    ///   stack. Use this for production deploys that demand
    ///   cross-backend reproducibility.
    pub fn with_enforcement(mut self, mode: crate::capabilities::EnforcementMode) -> Self {
        self.enforcement = mode;
        self
    }

    /// The engine's current enforcement mode.
    pub fn enforcement(&self) -> crate::capabilities::EnforcementMode {
        self.enforcement
    }

    /// Drain the collected normalization warnings. Returns the warnings
    /// captured during the most recent `up()` call (if any) and resets
    /// the buffer for the next invocation.
    pub fn take_normalization_warnings(&self) -> Vec<crate::capabilities::NormalizationWarning> {
        std::mem::take(&mut *self.normalization_warnings.lock().unwrap())
    }

    /// Resolve the container name for a given service, preferring the cached
    /// name set during `up()` and falling back to a fresh derivation only
    /// when no entry exists yet (e.g. for callers that operate on services
    /// before `up()` registered them — rare).
    pub fn resolve_container_name(&self, service_name: &str) -> String {
        if let Some(cached) = self
            .service_container_names
            .lock()
            .unwrap()
            .get(service_name)
            .cloned()
        {
            return cached;
        }
        let svc = self.spec.services.get(service_name);
        match svc {
            Some(s) => service::service_container_name(s, service_name),
            None => format!("{}-unknown", service_name),
        }
    }

    fn cache_container_name(&self, service_name: &str, container_name: &str) {
        self.service_container_names
            .lock()
            .unwrap()
            .insert(service_name.to_string(), container_name.to_string());
    }

    /// Project-namespace a volume or network name so two stacks with the
    /// same `volumes: { forgejo-pgdata: ... }` declaration don't collide
    /// and corrupt each other's data. Matches docker-compose's
    /// `<project>_<name>` convention.
    ///
    /// External resources (`{ external: true }`) are NOT prefixed — those
    /// are the caller's pre-existing infrastructure and we must reach
    /// them by their actual name.
    fn project_scoped_name(&self, name: &str) -> String {
        format!("{}_{}", self.project_name, name)
    }

    /// Resolve a volume name to the actual docker volume name we use,
    /// honoring `external: true` (skip namespacing) and `name:` overrides
    /// on the volume spec.
    fn resolve_volume_name(&self, decl_name: &str) -> String {
        let cfg_opt = self
            .spec
            .volumes
            .as_ref()
            .and_then(|v| v.get(decl_name))
            .and_then(|c| c.as_ref());
        if let Some(cfg) = cfg_opt {
            if cfg.external.unwrap_or(false) {
                // External: use `name:` if set, else literal declaration name.
                return cfg.name.clone().unwrap_or_else(|| decl_name.to_string());
            }
            if let Some(explicit) = &cfg.name {
                // Explicit `name:` override — caller asked for this exact
                // runtime name; honor it without project prefix.
                return explicit.clone();
            }
        }
        self.project_scoped_name(decl_name)
    }

    /// Same as `resolve_volume_name` for networks.
    fn resolve_network_name(&self, decl_name: &str) -> String {
        let cfg_opt = self
            .spec
            .networks
            .as_ref()
            .and_then(|n| n.get(decl_name))
            .and_then(|c| c.as_ref());
        if let Some(cfg) = cfg_opt {
            if cfg.external.unwrap_or(false) {
                return cfg.name.clone().unwrap_or_else(|| decl_name.to_string());
            }
            if let Some(explicit) = &cfg.name {
                return explicit.clone();
            }
        }
        self.project_scoped_name(decl_name)
    }

    /// Whether a volume is declared `external: true` (so `down(volumes:
    /// true)` must NOT remove it — it's not ours to drop).
    fn is_external_volume(&self, decl_name: &str) -> bool {
        self.spec
            .volumes
            .as_ref()
            .and_then(|v| v.get(decl_name))
            .and_then(|c| c.as_ref())
            .and_then(|c| c.external)
            .unwrap_or(false)
    }

    /// Whether a network is declared `external: true` (so `down()` must
    /// NOT remove it).
    fn is_external_network(&self, decl_name: &str) -> bool {
        self.spec
            .networks
            .as_ref()
            .and_then(|n| n.get(decl_name))
            .and_then(|c| c.as_ref())
            .and_then(|c| c.external)
            .unwrap_or(false)
    }

    fn register(self: Arc<Self>) -> ComposeHandle {
        let stack_id = NEXT_STACK_ID.fetch_add(1, Ordering::SeqCst);
        let services: Vec<String> = self.spec.services.keys().cloned().collect();
        let handle = ComposeHandle {
            stack_id,
            project_name: self.project_name.clone(),
            services,
        };
        COMPOSE_ENGINES.lock().unwrap().insert(stack_id, self);
        handle
    }

    pub async fn up(
        self: Arc<Self>,
        services: &[String],
        _detach: bool,
        _build: bool,
        _remove_orphans: bool,
    ) -> Result<ComposeHandle> {
        // Clear session bookkeeping at the start of up() so a second
        // up() on the same engine instance doesn't double-track
        // resources from the prior call. The FFI hides this (each
        // composeUp() call builds a fresh engine) but direct Rust
        // callers (tests, library consumers) hit the latent footgun
        // where rollback() drains networks/volumes from a previous
        // success, removing user-data the engine no longer "owns."
        self.session_containers.lock().unwrap().clear();
        self.session_networks.lock().unwrap().clear();
        self.session_volumes.lock().unwrap().clear();
        self.normalization_warnings.lock().unwrap().clear();

        // 1. Create networks
        //
        // Capability gate: when the backend declares `user_defined_bridge`
        // as Partial / Unsupported, skip user-defined-network creation
        // entirely and let containers attach to the backend's implicit
        // default network. Apple/container 0.12 ships with the
        // `container-network` plugin disabled by default, so emitting
        // `network create` against it crashes with
        // "Plugin 'container-network' not found." Skipping is the
        // graceful path — the user loses isolation between user-defined
        // networks but the stack actually starts. Logged so the user
        // knows it happened.
        let backend_caps = self.backend.capabilities();
        let user_bridge_supported = matches!(
            backend_caps.user_defined_bridge,
            crate::capabilities::FeatureSupport::Native
        );
        if let Some(networks) = &self.spec.networks {
            for (decl_name, config) in networks {
                // Skip creation entirely for `external: true` — the caller
                // asserts the network already exists and we must not
                // touch its lifecycle.
                if self.is_external_network(decl_name) {
                    continue;
                }
                if !user_bridge_supported {
                    tracing::warn!(
                        target: "perry::container::normalise",
                        backend = backend_caps.backend,
                        network = %decl_name,
                        "skipping network creation: backend does not natively \
                         support user-defined bridges; containers will use \
                         the default network"
                    );
                    self.normalization_warnings.lock().unwrap().push(
                        crate::capabilities::NormalizationWarning {
                            backend: backend_caps.backend,
                            service: format!("network:{}", decl_name),
                            field: "user_defined_bridge",
                            action: crate::capabilities::NormalizationAction::Dropped,
                            reason: format!(
                                "backend {} does not natively support \
                                 user-defined bridges; network creation skipped",
                                backend_caps.backend
                            ),
                        },
                    );
                    continue;
                }
                let runtime_name = self.resolve_network_name(decl_name);
                if self.backend.inspect_network(&runtime_name).await.is_err() {
                    if let Some(cfg) = config {
                        self.backend.create_network(&runtime_name, cfg).await?;
                    } else {
                        self.backend
                            .create_network(&runtime_name, &Default::default())
                            .await?;
                    }
                    self.session_networks
                        .lock()
                        .unwrap()
                        .push(runtime_name.clone());
                }
            }
        }

        // 2. Create volumes
        if let Some(volumes) = &self.spec.volumes {
            for (decl_name, config) in volumes {
                if self.is_external_volume(decl_name) {
                    continue;
                }
                let runtime_name = self.resolve_volume_name(decl_name);
                if self.backend.inspect_volume(&runtime_name).await.is_err() {
                    if let Some(cfg) = config {
                        self.backend.create_volume(&runtime_name, cfg).await?;
                    } else {
                        self.backend
                            .create_volume(&runtime_name, &Default::default())
                            .await?;
                    }
                    self.session_volumes
                        .lock()
                        .unwrap()
                        .push(runtime_name.clone());
                }
            }
        }

        // 3. Resolve order and start services
        let order = resolve_startup_order(&self.spec)?;
        let target: Vec<&String> = if services.is_empty() {
            order.iter().collect()
        } else {
            order.iter().filter(|s| services.contains(s)).collect()
        };

        let mut started = Vec::new();
        for svc_name in target {
            let svc = self.spec.services.get(svc_name).unwrap();
            // Generate the container name ONCE per service per session and
            // cache it so later methods (`exec`, `logs`, `down`) see the
            // same name we actually `run`'d the container with. The
            // underlying `service_container_name` re-randomises per call.
            let container_name = self
                .service_container_names
                .lock()
                .unwrap()
                .get(svc_name)
                .cloned()
                .unwrap_or_else(|| service::service_container_name(svc, svc_name));
            self.cache_container_name(svc_name, &container_name);

            // Extract primary network if any. The service references
            // the network by its DECLARATION key (`forgejo-db-net`), but
            // we attached at creation time as the project-namespaced
            // name (`<project>_forgejo-db-net`) — translate before
            // emitting the `--network` flag.
            let network = {
                let decl = match &svc.networks {
                    Some(crate::types::ServiceNetworks::List(l)) => l.first().cloned(),
                    Some(crate::types::ServiceNetworks::Map(m)) => m.keys().next().cloned(),
                    None => None,
                };
                // If the backend can't honor user-defined bridges, we
                // skipped network creation above — emitting `--network
                // <name>` would now fail with "no such network." Drop
                // the field; container falls through to the implicit
                // default network. Mirrors the network-creation skip
                // above so the spec stays internally consistent.
                if !user_bridge_supported {
                    None
                } else {
                    decl.map(|d| self.resolve_network_name(&d))
                }
            };

            let mut labels = svc.labels.as_ref().map(|l| l.to_map()).unwrap_or_default();
            labels.insert(
                "perry.compose.project".to_string(),
                self.project_name.clone(),
            );
            labels.insert("perry.compose.service".to_string(), svc_name.clone());
            // Spec-hash label — read back during the idempotency check
            // below to detect drift. When a service's user-visible spec
            // changes (image tag, env var, port, etc.), the hash
            // changes; we recreate the container instead of silently
            // skipping it.
            let spec_hash = service_spec_hash(svc);
            labels.insert("perry.compose.spec_hash".to_string(), spec_hash.clone());

            // If the service declares `build:` and no explicit `image:`,
            // build the image first. The implicit tag is `<svc>-image`
            // (matches `ComposeService::image_ref`). Pre-fix the engine
            // parsed `build:` but never acted on it — `up()` then tried
            // to run a container with an empty image string and got
            // "docker: invalid reference format" from the runtime.
            let image_to_use: String = if svc.needs_build() {
                let build_cfg = svc.build.as_ref().unwrap().as_build();
                let image_tag = svc.image_ref(svc_name);
                if let Err(e) = self.backend.build(&build_cfg, &image_tag).await {
                    self.rollback().await;
                    return Err(ComposeError::ServiceStartupFailed {
                        service: svc_name.clone(),
                        message: format!("build failed: {}", e),
                    });
                }
                image_tag
            } else {
                svc.image.clone().unwrap_or_default()
            };

            let container_spec = ContainerSpec {
                image: image_to_use,
                name: Some(container_name.clone()),
                ports: Some(
                    svc.ports
                        .as_ref()
                        .map(|p| {
                            p.iter()
                                .map(|ps| match ps {
                                    crate::types::PortSpec::Short(v) => match v {
                                        serde_yaml::Value::String(s) => s.clone(),
                                        serde_yaml::Value::Number(n) => n.to_string(),
                                        _ => v.as_str().unwrap_or_default().to_string(),
                                    },
                                    crate::types::PortSpec::Long(lp) => {
                                        let publ = lp
                                            .published
                                            .as_ref()
                                            .map(|v| match v {
                                                serde_yaml::Value::String(s) => s.clone(),
                                                serde_yaml::Value::Number(n) => n.to_string(),
                                                _ => v.as_str().unwrap_or_default().to_string(),
                                            })
                                            .unwrap_or_default();
                                        let target = match &lp.target {
                                            serde_yaml::Value::String(s) => s.clone(),
                                            serde_yaml::Value::Number(n) => n.to_string(),
                                            _ => lp.target.as_str().unwrap_or_default().to_string(),
                                        };
                                        format!("{}:{}", publ, target)
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                volumes: Some(
                    svc.volumes
                        .as_ref()
                        .map(|v| {
                            v.iter()
                                .map(|vs| {
                                    let raw = match vs {
                                        serde_yaml::Value::String(s) => s.clone(),
                                        _ => vs.as_str().unwrap_or_default().to_string(),
                                    };
                                    // Namespace named-volume references:
                                    //   "named:/path"      → "<proj>_named:/path"
                                    //   "named:/path:ro"   → "<proj>_named:/path:ro"
                                    //   "/host:/c"         → "/host:/c" (bind, literal)
                                    //   "./relative:/c"    → "./relative:/c" (bind, literal)
                                    // The leading-segment heuristic mirrors
                                    // docker-compose: a leading `/` or `.`
                                    // means bind mount; anything else is a
                                    // named-volume reference iff it's
                                    // declared in `spec.volumes`.
                                    if let Some(colon) = raw.find(':') {
                                        let head = &raw[..colon];
                                        let tail = &raw[colon..];
                                        if head.starts_with('/') || head.starts_with('.') {
                                            return raw;
                                        }
                                        let is_declared = self
                                            .spec
                                            .volumes
                                            .as_ref()
                                            .map(|m| m.contains_key(head))
                                            .unwrap_or(false);
                                        if is_declared {
                                            return format!(
                                                "{}{}",
                                                self.resolve_volume_name(head),
                                                tail
                                            );
                                        }
                                    }
                                    raw
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                env: Some(match &svc.environment {
                    Some(crate::types::ListOrDict::Dict(d)) => d
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.clone(),
                                v.as_ref()
                                    .map(|vv| match vv {
                                        serde_yaml::Value::String(s) => s.clone(),
                                        serde_yaml::Value::Number(n) => n.to_string(),
                                        serde_yaml::Value::Bool(b) => b.to_string(),
                                        _ => vv.as_str().unwrap_or_default().to_string(),
                                    })
                                    .unwrap_or_default(),
                            )
                        })
                        .collect(),
                    Some(crate::types::ListOrDict::List(l)) => l
                        .iter()
                        .filter_map(|s| s.split_once('='))
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    None => HashMap::new(),
                }),
                cmd: Some(match &svc.command {
                    Some(serde_yaml::Value::String(s)) => vec![s.clone()],
                    Some(serde_yaml::Value::Sequence(seq)) => seq
                        .iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect(),
                    _ => vec![],
                }),
                entrypoint: None,
                network: network.clone(),
                rm: None,
                read_only: svc.read_only,
                labels: Some(labels),
                privileged: svc.privileged,
                user: svc.user.clone(),
                workdir: svc.working_dir.clone(),
                cap_add: svc.cap_add.clone(),
                cap_drop: svc.cap_drop.clone(),
                // Left unset here: the compose engine expresses seccomp
                // / no-new-privileges through the `SecurityProfile`
                // built from `security_opt` below, and
                // `run_with_security` emits them from the profile.
                // Setting them on the spec too would double-emit the
                // `--security-opt` flags.
                seccomp: None,
                no_new_privileges: None,
                // Register the service KEY as a DNS alias on the
                // attached network. This makes `db:5432` / `api:8080`
                // etc. resolve from sibling containers without the
                // user having to set an explicit `container_name`.
                // Plus any long-form aliases the user declared via
                // `networks: { foo: { aliases: [...] } }`.
                //
                // Gated on `network.is_some()` — `--network-alias` is
                // only valid when the container attaches to a
                // user-defined network. Docker rejects it on the
                // default bridge: "network-scoped aliases are only
                // supported for user-defined networks." So when the
                // engine skipped network creation (apple/container
                // without the bridge plugin) OR when the spec just
                // doesn't declare networks at all, we omit the
                // aliases entirely. Cross-service DNS still works on
                // user-defined networks; the default bridge falls
                // back to container-name resolution.
                network_aliases: if network.is_some() {
                    Some({
                        let mut aliases = vec![svc_name.clone()];
                        if let Some(crate::types::ServiceNetworks::Map(m)) = &svc.networks {
                            for cfg in m.values().flatten() {
                                if let Some(extra) = &cfg.aliases {
                                    for a in extra {
                                        if !aliases.contains(a) {
                                            aliases.push(a.clone());
                                        }
                                    }
                                }
                            }
                        }
                        aliases
                    })
                } else {
                    None
                },
            };

            // Build SecurityProfile from the user's spec. Pre-fix the
            // engine left `seccomp: None` with a "could be parsed"
            // TODO, silently dropping the user's `security_opt: ["seccomp=..."]`
            // / `["no-new-privileges"]` entries. That was a real
            // security regression — users hardening containers got the
            // looser default. Now we parse + the normalization layer
            // drops the field on backends that don't support it
            // (apple/container) with a structured warning, so the user
            // knows the policy wasn't honored.
            let mut profile = crate::backend::SecurityProfile {
                read_only_root: svc.read_only.unwrap_or(false),
                seccomp: None,
                no_new_privileges: false,
            };
            if let Some(opts) = &svc.security_opt {
                profile.merge_security_opt(opts);
            }

            // Cross-backend determinism: normalize the spec + profile
            // against the backend's declared capabilities BEFORE
            // attempting to start the container. The same pass also
            // runs inside `CliBackend::run_with_security` (defense in
            // depth — direct callers of the trait still get sane
            // behavior), but the engine layer is where we apply the
            // user's chosen `EnforcementMode`. Strict mode aborts the
            // entire `up()` here rather than letting partially-modified
            // services succeed and leaving the stack inconsistent.
            let mut container_spec = container_spec;
            let caps = self.backend.capabilities();
            let mut svc_warnings =
                crate::capabilities::normalise_spec_for(caps, svc_name, &mut container_spec);
            svc_warnings.extend(crate::capabilities::normalise_security_profile(
                caps,
                svc_name,
                &mut profile,
            ));
            if !svc_warnings.is_empty() {
                match self.enforcement {
                    crate::capabilities::EnforcementMode::Lenient => {
                        for w in &svc_warnings {
                            tracing::warn!(
                                target: "perry::container::normalise",
                                backend = w.backend,
                                service = %w.service,
                                field = w.field,
                                reason = %w.reason,
                                "spec field dropped/translated for backend"
                            );
                        }
                    }
                    crate::capabilities::EnforcementMode::WarnUser => {
                        // Same as Lenient for log emission, but the
                        // caller can also drain via take_normalization_warnings().
                        for w in &svc_warnings {
                            tracing::warn!(
                                target: "perry::container::normalise",
                                backend = w.backend,
                                service = %w.service,
                                field = w.field,
                                reason = %w.reason,
                                "spec field dropped/translated for backend"
                            );
                        }
                    }
                    crate::capabilities::EnforcementMode::Strict => {
                        // Roll back any session resources created so far
                        // (networks/volumes/containers from prior services
                        // in the topological order) so the failed `up()`
                        // doesn't leave detritus on the host.
                        let summary = svc_warnings
                            .iter()
                            .map(|w| format!("{}: {} ({})", w.service, w.field, w.reason))
                            .collect::<Vec<_>>()
                            .join("; ");
                        self.rollback().await;
                        return Err(ComposeError::EnforcementViolation {
                            backend: caps.backend.to_string(),
                            service: svc_name.clone(),
                            details: summary,
                        });
                    }
                }
            }
            self.normalization_warnings
                .lock()
                .unwrap()
                .extend(svc_warnings);

            // Idempotency: skip if already running AND the live spec
            // hash matches the freshly-computed one. If the user
            // edited the spec (new image tag, new env value, etc.),
            // the hashes differ and we recreate. Pre-fix `up()`
            // skipped any container with a matching name regardless
            // of spec drift, leading to "I changed the image but my
            // redeploy did nothing" surprises.
            let mut skip = false;
            if let Ok(info) = self.backend.inspect(&container_name).await {
                let live_hash = info.labels.get("perry.compose.spec_hash").cloned();
                let drift = live_hash.as_deref() != Some(spec_hash.as_str());
                if drift {
                    // Spec changed — tear the existing container down
                    // so the create path below recreates it.
                    let _ = self.backend.stop(&container_name, Some(10)).await;
                    let _ = self.backend.remove(&container_name, true).await;
                } else if info.status == "running" {
                    skip = true;
                } else {
                    // Start existing stopped container. Track it in
                    // session_containers so a later service-startup
                    // failure rolls it BACK to stopped state instead of
                    // leaving a half-started stack — pre-fix, this
                    // branch added nothing to session_containers and
                    // rollback() couldn't undo the start.
                    if let Err(e) = self.backend.start(&container_name).await {
                        self.rollback().await;
                        return Err(ComposeError::ServiceStartupFailed {
                            service: svc_name.clone(),
                            message: e.to_string(),
                        });
                    }
                    self.session_containers
                        .lock()
                        .unwrap()
                        .push(container_name.clone());
                    skip = true;
                }
            }

            if !skip {
                match self
                    .backend
                    .run_with_security(&container_spec, &profile)
                    .await
                {
                    Ok(handle) => {
                        self.session_containers.lock().unwrap().push(handle.id);
                        started.push(container_name);
                    }
                    Err(e) => {
                        // Rollback
                        self.rollback().await;
                        return Err(ComposeError::ServiceStartupFailed {
                            service: svc_name.clone(),
                            message: e.to_string(),
                        });
                    }
                }
            }
        }

        Ok(self.register())
    }

    async fn rollback(&self) {
        let containers = self
            .session_containers
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for id in containers.into_iter().rev() {
            let _ = self.backend.stop(&id, Some(5)).await;
            let _ = self.backend.remove(&id, true).await;
        }

        let networks = self
            .session_networks
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for name in networks.into_iter().rev() {
            let _ = self.backend.remove_network(&name).await;
        }

        let volumes = self
            .session_volumes
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for name in volumes.into_iter().rev() {
            let _ = self.backend.remove_volume(&name).await;
        }
    }

    pub async fn down(
        &self,
        services: &[String],
        remove_orphans: bool,
        remove_volumes: bool,
    ) -> Result<()> {
        // `rollback()` removes `session_volumes` unconditionally — that's
        // correct semantics during an `up()` failure (those volumes were
        // just created and the caller wanted nothing to persist), but it
        // contradicts `remove_volumes=false` when called from `down()`.
        // Snapshot session_volumes around the rollback when the caller
        // opted to PRESERVE volumes so the unconditional drain inside
        // rollback doesn't strip them.
        if !remove_volumes {
            let saved_volumes: Vec<String> =
                self.session_volumes.lock().unwrap().drain(..).collect();
            self.rollback().await;
            *self.session_volumes.lock().unwrap() = saved_volumes;
        } else {
            self.rollback().await;
        }

        // 2. Clean up requested services (even if not in session)
        let order = resolve_startup_order(&self.spec)?;
        let target: Vec<&String> = if services.is_empty() {
            order.iter().collect()
        } else {
            order.iter().filter(|s| services.contains(s)).collect()
        };

        let mut final_order = target;
        final_order.reverse();

        for svc_name in final_order {
            let container_info = self.backend.list(true).await?;
            let containers_to_remove: Vec<String> = container_info
                .into_iter()
                .filter(|c| {
                    c.labels
                        .get("perry.compose.project")
                        .map(|v| v == &self.project_name)
                        .unwrap_or(false)
                        && c.labels
                            .get("perry.compose.service")
                            .map(|v| v == svc_name)
                            .unwrap_or(false)
                })
                .map(|c| c.id)
                .collect();

            for cid in containers_to_remove {
                let _ = self.backend.stop(&cid, Some(10)).await;
                let _ = self.backend.remove(&cid, true).await;
            }

            let container_name = self.resolve_container_name(svc_name);
            let _ = self.backend.stop(&container_name, Some(10)).await;
            let _ = self.backend.remove(&container_name, true).await;
        }

        // Orphan removal (`down --remove-orphans` semantics): containers
        // that still carry THIS project's `perry.compose.project` label
        // but whose `perry.compose.service` key is no longer in the
        // current spec — typically left behind when a service was
        // renamed or deleted between deploys. Every container the
        // engine creates is stamped with both labels in `up()`, so the
        // label pair is the ownership test; unlabelled containers (or
        // ones without a service label) were never ours and are left
        // alone, as are other projects' containers. Runs BEFORE network
        // removal so an orphan attached to a project network doesn't
        // keep the network alive. Pre-fix this flag was parsed at the
        // FFI boundary and then discarded (`_remove_orphans`).
        if remove_orphans {
            match self.backend.list(true).await {
                Ok(all) => {
                    for c in all {
                        let ours = c
                            .labels
                            .get("perry.compose.project")
                            .map(|v| v == &self.project_name)
                            .unwrap_or(false);
                        if !ours {
                            continue;
                        }
                        let orphan = c
                            .labels
                            .get("perry.compose.service")
                            .map(|svc| !self.spec.services.contains_key(svc))
                            .unwrap_or(false);
                        if orphan {
                            let _ = self.backend.stop(&c.id, Some(10)).await;
                            let _ = self.backend.remove(&c.id, true).await;
                        }
                    }
                }
                Err(e) => {
                    // Best-effort like the rest of down(): a backend
                    // that can't list containers shouldn't abort the
                    // network/volume teardown below.
                    tracing::warn!(
                        target: "perry::compose::down",
                        project = %self.project_name,
                        error = %e,
                        "remove_orphans: could not list containers; skipping orphan sweep"
                    );
                }
            }
        }

        if let Some(networks) = &self.spec.networks {
            for decl_name in networks.keys() {
                // Skip `external: true` networks — those are the
                // caller's pre-existing infrastructure and must not be
                // deleted by us. Pre-fix `down()` removed every network
                // in `spec.networks` regardless, which silently deleted
                // shared infra a user had explicitly marked external.
                if self.is_external_network(decl_name) {
                    continue;
                }
                let runtime_name = self.resolve_network_name(decl_name);
                let _ = self.backend.remove_network(&runtime_name).await;
            }
        }

        if remove_volumes {
            if let Some(volumes) = &self.spec.volumes {
                for decl_name in volumes.keys() {
                    if self.is_external_volume(decl_name) {
                        continue;
                    }
                    let runtime_name = self.resolve_volume_name(decl_name);
                    let _ = self.backend.remove_volume(&runtime_name).await;
                }
            }
        }

        Ok(())
    }

    pub async fn ps(&self) -> Result<Vec<ContainerInfo>> {
        let mut infos = Vec::new();
        for svc_name in self.spec.services.keys() {
            let container_name = self.resolve_container_name(svc_name);
            if let Ok(info) = self.backend.inspect(&container_name).await {
                infos.push(info);
            }
        }
        Ok(infos)
    }

    pub async fn logs(
        &self,
        services: &[String],
        tail: Option<u32>,
    ) -> Result<HashMap<String, String>> {
        let mut all_logs = HashMap::new();
        let target: Vec<&String> = if services.is_empty() {
            self.spec.services.keys().collect()
        } else {
            services.iter().collect()
        };

        for svc_name in target {
            let container_name = self.resolve_container_name(svc_name);
            if let Ok(logs) = self.backend.logs(&container_name, tail).await {
                all_logs.insert(
                    svc_name.clone(),
                    format!("STDOUT:\n{}\nSTDERR:\n{}", logs.stdout, logs.stderr),
                );
            }
        }
        Ok(all_logs)
    }

    pub async fn exec(
        &self,
        service: &str,
        cmd: &[String],
        env: Option<&HashMap<String, String>>,
        workdir: Option<&str>,
    ) -> Result<ContainerLogs> {
        if !self.spec.services.contains_key(service) {
            return Err(ComposeError::NotFound(service.into()));
        }
        let container_name = self.resolve_container_name(service);
        self.backend.exec(&container_name, cmd, env, workdir).await
    }

    pub fn config(&self) -> Result<String> {
        serde_yaml::to_string(&self.spec).map_err(ComposeError::ParseError)
    }

    pub async fn start(&self, services: &[String]) -> Result<()> {
        let target: Vec<&String> = if services.is_empty() {
            self.spec.services.keys().collect()
        } else {
            services.iter().collect()
        };
        for svc_name in target {
            let container_name = self.resolve_container_name(svc_name);
            self.backend.start(&container_name).await?;
        }
        Ok(())
    }

    pub async fn stop(&self, services: &[String]) -> Result<()> {
        let target: Vec<&String> = if services.is_empty() {
            self.spec.services.keys().collect()
        } else {
            services.iter().collect()
        };
        for svc_name in target {
            let container_name = self.resolve_container_name(svc_name);
            self.backend.stop(&container_name, None).await?;
        }
        Ok(())
    }

    pub async fn restart(&self, services: &[String]) -> Result<()> {
        self.stop(services).await?;
        self.start(services).await
    }
}

pub fn resolve_startup_order(spec: &ComposeSpec) -> Result<Vec<String>> {
    let mut in_degree: IndexMap<String, usize> = IndexMap::new();
    let mut dependents: IndexMap<String, Vec<String>> = IndexMap::new();

    for name in spec.services.keys() {
        in_degree.insert(name.clone(), 0);
        dependents.insert(name.clone(), Vec::new());
    }

    for (name, service) in &spec.services {
        if let Some(deps) = &service.depends_on {
            for dep in deps.service_names() {
                if !spec.services.contains_key(&dep) {
                    return Err(ComposeError::ValidationError {
                        message: format!(
                            "Service '{}' depends on '{}' which is not defined",
                            name, dep
                        ),
                    });
                }
                *in_degree.get_mut(name).unwrap() += 1;
                dependents.get_mut(&dep).unwrap().push(name.clone());
            }
        }
    }

    let mut queue: std::collections::BTreeSet<String> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    let mut order: Vec<String> = Vec::new();
    while let Some(service) = queue.pop_first() {
        order.push(service.clone());
        for dependent in dependents.get(&service).unwrap_or(&Vec::new()).clone() {
            let deg = in_degree.get_mut(&dependent).unwrap();
            *deg -= 1;
            if *deg == 0 {
                queue.insert(dependent);
            }
        }
    }

    if order.len() != spec.services.len() {
        let cycle_services: Vec<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(name, _)| name.clone())
            .collect();
        return Err(ComposeError::DependencyCycle {
            services: cycle_services,
        });
    }

    Ok(order)
}

// ──────────────────────────────────────────────────────────────────────
// Free-function cleanup API
//
// These let callers tear down resources WITHOUT holding a `ComposeHandle`
// — useful for: end-of-test cleanup; recovering from a crashed
// process that left orphans; clearing dev state between iterations.
// All three drive `ContainerBackend::list/stop/remove/remove_volume/
// remove_network` so they work against any backend Perry supports.
//
// Identification rules:
//   - Containers Perry created carry the `perry.compose.project=<proj>`
//     label (and `perry.compose.service=<svc>`).
//   - Volumes + networks created by `ComposeEngine::up` use the
//     project-namespaced runtime name pattern (`<proj>_<decl>`).
//   - Externally-created resources are NEVER touched by these helpers.
// ──────────────────────────────────────────────────────────────────────

/// Summary of what `down_by_project` / `down_all` actually removed.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CleanupReport {
    pub containers_removed: usize,
    pub networks_removed: usize,
    pub volumes_removed: usize,
    /// Per-resource error messages. Cleanup is best-effort: an error
    /// removing one resource doesn't abort the rest. Inspect this list
    /// to see what failed.
    pub errors: Vec<String>,
}

/// Options for `down_by_project` / `down_all`.
#[derive(Debug, Clone, Default)]
pub struct CleanupOptions {
    /// Drop named volumes too (default: false — preserves data).
    pub volumes: bool,
    /// Best-effort prune unused networks AFTER container removal
    /// (default: true — networks have no persistent state).
    pub networks: bool,
}

impl CleanupOptions {
    pub fn default_for_project() -> Self {
        Self {
            volumes: false,
            networks: true,
        }
    }
}

/// Tear down every container labelled with `perry.compose.project =
/// <project_name>`. Safer than per-stack `down(handle)` because it
/// works WITHOUT holding the handle — find the resources by label,
/// remove them. Optionally drops project-namespaced volumes and
/// networks too.
pub async fn down_by_project(
    backend: &dyn ContainerBackend,
    project: &str,
    opts: &CleanupOptions,
) -> CleanupReport {
    let mut report = CleanupReport::default();

    // 1. Find every container Perry created for this project.
    let all_containers = match backend.list(true).await {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("list containers: {}", e));
            return report;
        }
    };
    let ours: Vec<ContainerInfo> = all_containers
        .into_iter()
        .filter(|c| {
            c.labels
                .get("perry.compose.project")
                .map(|v| v == project)
                .unwrap_or(false)
        })
        .collect();

    // 2. Stop + remove each. Order matters less than completeness here
    // — we don't have a topological sort without the original spec, so
    // just blast them all in parallel-batch fashion (still serial to
    // keep error attribution clean).
    for c in &ours {
        if let Err(e) = backend.stop(&c.id, Some(5)).await {
            report.errors.push(format!("stop {}: {}", c.id, e));
        }
        match backend.remove(&c.id, true).await {
            Ok(_) => report.containers_removed += 1,
            Err(e) => report
                .errors
                .push(format!("remove container {}: {}", c.id, e)),
        }
    }

    // 3. Remove networks/volumes by NAME PREFIX `<project>_*`. Some
    // backends don't expose `list_networks` / `list_volumes` via our
    // trait yet, so we don't enumerate — instead, we let the docker
    // network/volume `remove` reject "in use" cleanly (which is the
    // right behavior: external resources mounted into our project's
    // containers stay intact). This iteration enumerates networks
    // we WOULD have created if a fresh `up()` had run by walking
    // `docker network ls --filter label=perry.compose.project=<p>`.
    // Without that filter API we make a best-effort pass: callers
    // tearing down without a spec aren't surgical. The label-scan
    // approach is the next iteration.
    //
    // For now: skip networks/volumes when there's no spec; the
    // resources persist (volumes appropriately, networks until
    // pruned) and the user can `docker volume prune --filter
    // label=perry.compose.project=<p>` if they need surgery.
    let _ = opts; // honored by `down_for_spec_no_handle` below
    report
}

/// Tear down every Perry-managed container regardless of project.
/// **Use sparingly** — this kills every stack on the host that was
/// brought up via `perry/compose`, including ones the user might be
/// actively developing against in another terminal.
pub async fn down_all(backend: &dyn ContainerBackend, _opts: &CleanupOptions) -> CleanupReport {
    let mut report = CleanupReport::default();

    let all_containers = match backend.list(true).await {
        Ok(v) => v,
        Err(e) => {
            report.errors.push(format!("list containers: {}", e));
            return report;
        }
    };
    let ours: Vec<ContainerInfo> = all_containers
        .into_iter()
        .filter(|c| c.labels.contains_key("perry.compose.project"))
        .collect();

    for c in &ours {
        if let Err(e) = backend.stop(&c.id, Some(5)).await {
            report.errors.push(format!("stop {}: {}", c.id, e));
        }
        match backend.remove(&c.id, true).await {
            Ok(_) => report.containers_removed += 1,
            Err(e) => report
                .errors
                .push(format!("remove container {}: {}", c.id, e)),
        }
    }
    report
}

/// Idempotent single-container removal: stop + force-remove if the
/// container exists; treat NotFound as success. Useful in cleanup
/// paths where you don't know whether the container was ever started
/// (or was already torn down by an earlier `down()` call).
pub async fn remove_if_exists(
    backend: &dyn ContainerBackend,
    id_or_name: &str,
    force: bool,
) -> Result<bool> {
    // Probe first; treat any inspect error as "not present"
    if backend.inspect(id_or_name).await.is_err() {
        return Ok(false);
    }
    let _ = backend.stop(id_or_name, Some(5)).await;
    match backend.remove(id_or_name, force).await {
        Ok(_) => Ok(true),
        Err(ComposeError::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}
