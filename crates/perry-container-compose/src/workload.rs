//! Workload graph execution engine.

use crate::backend::ContainerBackend;
use crate::error::{ComposeError, Result};
use crate::types::{ContainerInfo, ContainerLogs, ContainerSpec};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

// ============ Types ============

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RuntimeSpec {
    Oci,
    Microvm { config: Option<serde_json::Value> },
    Wasm { module: Option<String> },
    Auto,
}

impl Default for RuntimeSpec {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PolicyTier {
    Default,
    Isolated,
    Hardened,
    Untrusted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySpec {
    pub tier: PolicyTier,
    #[serde(default)]
    pub no_network: bool,
    #[serde(default)]
    pub read_only_root: bool,
    #[serde(default)]
    pub seccomp: bool,
}

impl Default for PolicySpec {
    fn default() -> Self {
        Self {
            tier: PolicyTier::Default,
            no_network: false,
            read_only_root: false,
            seccomp: false,
        }
    }
}

impl PolicySpec {
    /// Apply tier-based defaults on top of explicit per-flag overrides.
    ///
    /// The tier sets a floor; explicitly-set fields on the user's `PolicySpec`
    /// can lift it but never below. Used by `WorkloadGraphEngine::run` to
    /// compute the actual `SecurityProfile` + `ContainerSpec` adjustments.
    ///
    /// - `Default`     — no defaults; user values are honored verbatim.
    /// - `Isolated`    — `no_network=true` (cross-node networking disabled).
    /// - `Hardened`    — `read_only_root=true`, `seccomp=true`.
    /// - `Untrusted`   — `Hardened` + `no_network=true` + (caller-side) forces
    ///                   the runtime to `MicroVm` for kernel isolation.
    pub fn effective(&self) -> Self {
        let mut out = self.clone();
        match self.tier {
            PolicyTier::Default => {}
            PolicyTier::Isolated => {
                out.no_network = true;
            }
            PolicyTier::Hardened => {
                out.read_only_root = true;
                out.seccomp = true;
            }
            PolicyTier::Untrusted => {
                out.read_only_root = true;
                out.seccomp = true;
                out.no_network = true;
            }
        }
        // User-explicit `true` is preserved (we only set, never clear).
        out.no_network |= self.no_network;
        out.read_only_root |= self.read_only_root;
        out.seccomp |= self.seccomp;
        out
    }

    /// Whether this policy requires the runtime to provide kernel-level
    /// isolation (i.e. a microVM rather than a shared-kernel container).
    /// `Untrusted` tier is the canonical case.
    pub fn requires_microvm(&self) -> bool {
        matches!(self.tier, PolicyTier::Untrusted)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefProjection {
    Endpoint,
    Ip,
    InternalUrl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRef {
    pub node_id: String,
    pub projection: RefProjection,
    pub port: Option<String>,
}

impl WorkloadRef {
    pub fn resolve(
        &self,
        running_nodes: &HashMap<String, ContainerInfo>,
    ) -> std::result::Result<String, String> {
        let info = running_nodes
            .get(&self.node_id)
            .ok_or_else(|| format!("Node {} not found", self.node_id))?;
        let host = if !info.ip_address.is_empty() {
            &info.ip_address
        } else {
            &info.id
        };

        match self.projection {
            RefProjection::Endpoint => {
                let port = self.port.as_deref().unwrap_or("80");
                Ok(format!("{}:{}", host, port))
            }
            RefProjection::Ip => Ok(host.clone()),
            RefProjection::InternalUrl => {
                let port = self.port.as_deref().unwrap_or("80");
                Ok(format!("http://{}:{}", host, port))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkloadEnvValue {
    Literal(String),
    Ref(WorkloadRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadNode {
    pub id: String,
    pub name: String,
    pub image: Option<String>,
    pub resources: Option<serde_json::Value>,
    pub ports: Vec<String>,
    pub env: HashMap<String, WorkloadEnvValue>,
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub runtime: RuntimeSpec,
    #[serde(default)]
    pub policy: PolicySpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadEdge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadGraph {
    pub name: String,
    pub nodes: IndexMap<String, WorkloadNode>,
    pub edges: Vec<WorkloadEdge>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStrategy {
    Sequential,
    MaxParallel,
    DependencyAware,
    ParallelSafe,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FailureStrategy {
    RollbackAll,
    PartialContinue,
    HaltGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGraphOptions {
    pub strategy: ExecutionStrategy,
    pub on_failure: FailureStrategy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NodeState {
    Running,
    Stopped,
    Failed,
    Pending,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStatus {
    pub nodes: HashMap<String, NodeState>,
    pub healthy: bool,
    pub errors: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub node_id: String,
    pub name: String,
    pub container_id: Option<String>,
    pub state: NodeState,
    pub image: Option<String>,
    pub ip_address: Option<String>,
}

// ============ Engine ============

pub struct WorkloadGraphEngine {
    pub graph: WorkloadGraph,
    pub backend: Arc<dyn ContainerBackend>,
    pub running_containers: Mutex<HashMap<String, String>>, // node_id -> container_id
}

static WORKLOAD_INSTANCES: Lazy<Mutex<IndexMap<u64, Arc<WorkloadGraphEngine>>>> =
    Lazy::new(|| Mutex::new(IndexMap::new()));

static NEXT_WORKLOAD_ID: AtomicU64 = AtomicU64::new(1);

impl WorkloadGraphEngine {
    pub fn new(graph: WorkloadGraph, backend: Arc<dyn ContainerBackend>) -> Self {
        Self {
            graph,
            backend,
            running_containers: Mutex::new(HashMap::new()),
        }
    }

    pub async fn run(&self, options: RunGraphOptions) -> Result<u64> {
        let order = self.resolve_execution_order()?;

        let mut running = self.running_containers.lock().await;
        let mut info_cache = HashMap::new();

        for node_id in order {
            let node = self.graph.nodes.get(&node_id).unwrap();

            // Resolve environment variables (handling refs)
            let mut env = HashMap::new();
            for (key, val) in &node.env {
                match val {
                    WorkloadEnvValue::Literal(s) => {
                        env.insert(key.clone(), s.clone());
                    }
                    WorkloadEnvValue::Ref(r) => {
                        let resolved = r
                            .resolve(&info_cache)
                            .map_err(|e| ComposeError::ValidationError { message: e })?;
                        env.insert(key.clone(), resolved);
                    }
                }
            }

            let mut labels = HashMap::new();
            labels.insert("perry.workload.name".into(), self.graph.name.clone());
            labels.insert("perry.workload.node".into(), node_id.clone());
            labels.insert(
                "perry.workload.policyTier".into(),
                format!("{:?}", node.policy.tier).to_ascii_lowercase(),
            );

            // Apply tier-based defaults on top of user-explicit flags. The
            // returned `PolicySpec` is the canonical decision: every per-tier
            // hardening lives here so the spec construction below stays
            // straightforward.
            let policy = node.policy.effective();

            // `Untrusted` requires kernel-level isolation. Today the CLI
            // backend doesn't provide microVM containers; surface a clear
            // error so the caller can pick a backend that does (e.g. a
            // future Lima/Firecracker integration). `RuntimeSpec::MicroVm`
            // declared on the node is the explicit opt-in for that path —
            // when the backend supports it we'll route there; until then,
            // returning `BackendNotAvailable` makes the missing capability
            // visible instead of silently dropping the isolation guarantee.
            if policy.requires_microvm() && !matches!(node.runtime, RuntimeSpec::Microvm { .. }) {
                if std::env::var("PERRY_ALLOW_UNTRUSTED_SHARED_KERNEL").is_err() {
                    return Err(ComposeError::BackendNotAvailable {
                        name: self.backend.backend_name().to_string(),
                        reason: format!(
                            "node '{}' has policy tier 'untrusted' which requires \
                             microVM isolation, but the active backend doesn't \
                             expose one. Either select RuntimeSpec::MicroVm \
                             explicitly on the node or set \
                             PERRY_ALLOW_UNTRUSTED_SHARED_KERNEL=1 to opt out \
                             (NOT recommended for actually-untrusted code).",
                            node_id
                        ),
                    });
                }
            }

            let spec = ContainerSpec {
                image: node.image.clone().unwrap_or_default(),
                name: Some(format!("{}-{}", self.graph.name, node.name)),
                ports: Some(node.ports.clone()),
                env: Some(env),
                rm: Some(true),
                read_only: Some(policy.read_only_root),
                labels: Some(labels),
                // `no_network=true` → use the runtime's "none" network so
                // the container has no external + no inter-container
                // connectivity. CNI runtimes interpret literal "none" as
                // the disabled-bridge sentinel (Docker, podman, apple
                // /container all honor this).
                network: if policy.no_network {
                    Some("none".into())
                } else {
                    None
                },
                ..Default::default()
            };

            let profile = crate::backend::SecurityProfile {
                read_only_root: policy.read_only_root,
                seccomp: if policy.seccomp {
                    Some("default".into())
                } else {
                    None
                },
                ..Default::default()
            };

            match self.backend.run_with_security(&spec, &profile).await {
                Ok(handle) => {
                    running.insert(node_id.clone(), handle.id.clone());
                    // Inspect to get IP/etc for future refs
                    if let Ok(info) = self.backend.inspect(&handle.id).await {
                        info_cache.insert(node_id.clone(), info);
                    }
                }
                Err(e) => {
                    if options.on_failure == FailureStrategy::RollbackAll {
                        // Rollback logic here
                        for (_, cid) in running.iter() {
                            let _ = self.backend.stop(cid, Some(5)).await;
                            let _ = self.backend.remove(cid, true).await;
                        }
                    }
                    return Err(ComposeError::ServiceStartupFailed {
                        service: node_id,
                        message: e.to_string(),
                    });
                }
            }
        }

        let id = NEXT_WORKLOAD_ID.fetch_add(1, Ordering::SeqCst);
        Ok(id)
    }

    fn resolve_execution_order(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();

        for node_id in self.graph.nodes.keys() {
            in_degree.insert(node_id.clone(), 0);
            dependents.insert(node_id.clone(), Vec::new());
        }

        for (node_id, node) in &self.graph.nodes {
            for dep in &node.depends_on {
                if !self.graph.nodes.contains_key(dep) {
                    return Err(ComposeError::ValidationError {
                        message: format!(
                            "Node '{}' depends on '{}' which is not in graph",
                            node_id, dep
                        ),
                    });
                }
                *in_degree.get_mut(node_id).unwrap() += 1;
                dependents.get_mut(dep).unwrap().push(node_id.clone());
            }
        }

        let mut queue: std::collections::VecDeque<String> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(id, _)| id.clone())
            .collect();

        // Sort for deterministic order
        let mut queue_vec: Vec<String> = queue.into_iter().collect();
        queue_vec.sort();
        queue = queue_vec.into();

        let mut order = Vec::new();
        while let Some(id) = queue.pop_front() {
            order.push(id.clone());
            for dependent in dependents.get(&id).unwrap_or(&Vec::new()) {
                let deg = in_degree.get_mut(dependent).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(dependent.clone());
                }
            }
        }

        if order.len() != self.graph.nodes.len() {
            let cycle: Vec<String> = in_degree
                .into_iter()
                .filter(|(_, d)| *d > 0)
                .map(|(id, _)| id)
                .collect();
            return Err(ComposeError::DependencyCycle { services: cycle });
        }

        Ok(order)
    }

    pub async fn status(&self) -> Result<GraphStatus> {
        let running = self.running_containers.lock().await;
        let mut nodes = HashMap::new();
        let mut healthy = true;
        let mut errors = HashMap::new();

        for node_id in self.graph.nodes.keys() {
            if let Some(cid) = running.get(node_id) {
                match self.backend.inspect(cid).await {
                    Ok(info) => {
                        let state = if info.status == "running" {
                            NodeState::Running
                        } else {
                            healthy = false;
                            NodeState::Stopped
                        };
                        nodes.insert(node_id.clone(), state);
                    }
                    Err(e) => {
                        healthy = false;
                        nodes.insert(node_id.clone(), NodeState::Failed);
                        errors.insert(node_id.clone(), e.to_string());
                    }
                }
            } else {
                nodes.insert(node_id.clone(), NodeState::Pending);
            }
        }

        Ok(GraphStatus {
            nodes,
            healthy,
            errors,
        })
    }

    pub async fn down(&self, force: bool) -> Result<()> {
        let mut running = self.running_containers.lock().await;

        // 1. Clean up containers we have handles for in this session
        for (_, cid) in running.drain() {
            let _ = self.backend.stop(&cid, Some(10)).await;
            let _ = self.backend.remove(&cid, force).await;
        }

        // 2. Clean up any orphans by label
        if let Ok(all) = self.backend.list(true).await {
            for container in all {
                if container
                    .labels
                    .get("perry.workload.name")
                    .map(|v| v == &self.graph.name)
                    .unwrap_or(false)
                {
                    let _ = self.backend.stop(&container.id, Some(10)).await;
                    let _ = self.backend.remove(&container.id, force).await;
                }
            }
        }
        Ok(())
    }

    pub async fn logs(&self, node_id: &str, tail: Option<u32>) -> Result<ContainerLogs> {
        let running = self.running_containers.lock().await;
        let cid = running
            .get(node_id)
            .ok_or_else(|| ComposeError::NotFound(node_id.into()))?;
        self.backend.logs(cid, tail).await
    }

    pub async fn exec(&self, node_id: &str, cmd: &[String]) -> Result<ContainerLogs> {
        let running = self.running_containers.lock().await;
        let cid = running
            .get(node_id)
            .ok_or_else(|| ComposeError::NotFound(node_id.into()))?;
        self.backend.exec(cid, cmd, None, None).await
    }

    pub async fn ps(&self) -> Result<Vec<NodeInfo>> {
        let running = self.running_containers.lock().await;
        let mut infos = Vec::new();
        for (node_id, node) in &self.graph.nodes {
            let cid = running.get(node_id).cloned();
            let mut state = NodeState::Pending;
            let mut ip_address = None;
            if let Some(ref id) = cid {
                if let Ok(info) = self.backend.inspect(id).await {
                    state = if info.status == "running" {
                        NodeState::Running
                    } else {
                        NodeState::Stopped
                    };
                    if !info.ip_address.is_empty() {
                        ip_address = Some(info.ip_address.clone());
                    }
                } else {
                    state = NodeState::Failed;
                }
            }
            infos.push(NodeInfo {
                node_id: node_id.clone(),
                name: node.name.clone(),
                container_id: cid,
                state,
                image: node.image.clone(),
                ip_address,
            });
        }
        Ok(infos)
    }
}

pub async fn register_workload_engine(engine: Arc<WorkloadGraphEngine>) -> u64 {
    let id = NEXT_WORKLOAD_ID.fetch_add(1, Ordering::SeqCst);
    WORKLOAD_INSTANCES.lock().await.insert(id, engine);
    id
}

pub async fn get_workload_engine(id: u64) -> Option<Arc<WorkloadGraphEngine>> {
    WORKLOAD_INSTANCES.lock().await.get(&id).cloned()
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    #[test]
    fn default_tier_keeps_user_flags_verbatim() {
        let p = PolicySpec {
            tier: PolicyTier::Default,
            no_network: false,
            read_only_root: false,
            seccomp: false,
        };
        let eff = p.effective();
        assert!(!eff.no_network);
        assert!(!eff.read_only_root);
        assert!(!eff.seccomp);
        assert!(!eff.requires_microvm());
    }

    #[test]
    fn isolated_tier_forces_no_network() {
        let p = PolicySpec {
            tier: PolicyTier::Isolated,
            ..PolicySpec::default()
        };
        let eff = p.effective();
        assert!(
            eff.no_network,
            "Isolated must disable cross-node networking"
        );
        assert!(!eff.requires_microvm());
    }

    #[test]
    fn hardened_tier_forces_read_only_and_seccomp() {
        let p = PolicySpec {
            tier: PolicyTier::Hardened,
            ..PolicySpec::default()
        };
        let eff = p.effective();
        assert!(eff.read_only_root);
        assert!(eff.seccomp);
        assert!(!eff.no_network, "Hardened keeps networking by default");
        assert!(!eff.requires_microvm());
    }

    #[test]
    fn untrusted_tier_forces_full_isolation_and_microvm() {
        let p = PolicySpec {
            tier: PolicyTier::Untrusted,
            ..PolicySpec::default()
        };
        let eff = p.effective();
        assert!(eff.read_only_root);
        assert!(eff.seccomp);
        assert!(eff.no_network);
        assert!(
            eff.requires_microvm(),
            "Untrusted demands kernel-level isolation"
        );
    }

    #[test]
    fn user_flags_are_never_cleared_by_lower_tier() {
        // Default tier with user explicitly setting no_network should still
        // produce no_network=true after effective() applies tier defaults.
        let p = PolicySpec {
            tier: PolicyTier::Default,
            no_network: true,
            read_only_root: true,
            seccomp: true,
        };
        let eff = p.effective();
        assert!(eff.no_network);
        assert!(eff.read_only_root);
        assert!(eff.seccomp);
    }
}
