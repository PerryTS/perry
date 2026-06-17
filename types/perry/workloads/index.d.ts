/**
 * perry/workloads — workload-graph orchestration (ALPHA)
 *
 * ⚠ **ALPHA — NOT PRODUCTION-READY**
 *
 * This module exposes the `WorkloadGraphEngine` API for orchestrating
 * typed DAGs of `WorkloadNode`s with per-node runtime selection
 * (`oci` / `microvm` / `wasm` / `auto`) and explicit policy tiers
 * (`default` / `isolated` / `hardened` / `untrusted`).
 *
 * Not-yet-shipped functionality:
 *   - `ExecutionStrategy::ParallelSafe` / `MaxParallel` are not yet
 *     implemented; only `Sequential` is honored.
 *   - Edge-condition `service_healthy` waiting is not implemented.
 *   - `RuntimeSpec::Microvm` and `Wasm` have no concrete backend yet
 *     (the runtime returns `BackendNotAvailable` for `policy.tier =
 *     "untrusted"` unless `PERRY_ALLOW_UNTRUSTED_SHARED_KERNEL=1`).
 *   - No integration tests are gating regressions today.
 *
 * **Recommendation:** for production multi-service deploys today,
 * use [`perry/compose`](perry/compose). Switch to `perry/workloads`
 * once this notice is removed.
 *
 * Tracking issue: see SPEC.md §11.1 and the audit notes in
 * `.kiro/specs/alloy-container/requirements.md` Implementation Notes
 * section.
 *
 * @module perry/workloads
 * @alpha
 */

// ============ Configuration types ============

/** Runtime selector for a workload node. */
export type RuntimeSpec =
  | { type: "auto" }
  | { type: "oci"; config?: object }
  | { type: "microvm"; config?: object }   // ⚠ no concrete backend yet
  | { type: "wasm"; module?: string };     // ⚠ no concrete backend yet

/**
 * Helper constructors for `RuntimeSpec` values.
 *
 * @alpha
 */
export const runtime: {
  auto():    RuntimeSpec;
  oci():     RuntimeSpec;
  microvm(): RuntimeSpec;
  wasm():    RuntimeSpec;
};

/** Per-node isolation tier. */
export type PolicyTier = "default" | "isolated" | "hardened" | "untrusted";

export interface PolicySpec {
  tier: PolicyTier;
  /** Disable cross-node networking */
  noNetwork?: boolean;
  /** Mount the root filesystem read-only */
  readOnlyRoot?: boolean;
  /** Apply the runtime's default seccomp profile */
  seccomp?: boolean;
}

/**
 * Helper constructors for `PolicySpec` values.
 *
 * @alpha
 */
export const policy: {
  default():   PolicySpec;
  isolated():  PolicySpec;
  hardened():  PolicySpec;
  untrusted(): PolicySpec;
};

// ============ Workload graph types ============

/** Reference projection for cross-node values. */
export type RefProjection = "endpoint" | "ip" | "internalUrl";

export interface WorkloadRef {
  nodeId: string;
  projection: RefProjection;
  port?: string;
}

export type WorkloadEnvValue = string | WorkloadRef;

export interface WorkloadNode {
  id: string;
  name: string;
  image?: string;
  ports?: string[];
  env?: Record<string, WorkloadEnvValue>;
  dependsOn?: string[];
  runtime?: RuntimeSpec;
  policy?: PolicySpec;
}

export interface WorkloadEdge {
  from: string;
  to: string;
  condition?: string;
}

export interface WorkloadGraph {
  name: string;
  nodes: Record<string, WorkloadNode>;
  edges?: WorkloadEdge[];
}

// ============ Execution options ============

export type ExecutionStrategy =
  | "sequential"          // ✅ implemented
  | "maxParallel"         // ⚠ alpha — falls back to sequential
  | "dependencyAware"     // ⚠ alpha — falls back to sequential
  | "parallelSafe";       // ⚠ alpha — falls back to sequential

export type FailureStrategy = "rollbackAll" | "partialContinue" | "haltGraph";

export interface RunGraphOptions {
  strategy?: ExecutionStrategy;
  onFailure?: FailureStrategy;
}

export interface NodeInfo {
  nodeId: string;
  name: string;
  containerId?: string;
  state: "running" | "stopped" | "failed" | "pending" | "unknown";
  image?: string;
}

export interface GraphStatus {
  nodes: Record<string, NodeInfo["state"]>;
  healthy: boolean;
  errors: Record<string, string>;
}

// ============ API ============

/**
 * Construct a `WorkloadGraph` value (does not run it).
 *
 * @alpha
 */
export function graph(name: string, nodes: Record<string, WorkloadNode>): string;

/**
 * Construct a `WorkloadNode` value.
 *
 * @alpha
 */
export function node(name: string, spec: WorkloadNode): string;

/**
 * Run a workload graph. Returns an opaque integer handle.
 *
 * @alpha
 */
export function runGraph(
  graphJson: string,
  options?: RunGraphOptions,
): Promise<number>;

/**
 * Inspect a graph WITHOUT starting any nodes — returns a JSON-encoded
 * `GraphStatus` string.
 *
 * @alpha
 */
export function inspectGraph(graphJson: string): Promise<string>;
