// Type declarations for perry/container — Perry's OCI container management module
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/container"`.

// ---------------------------------------------------------------------------
// Container Lifecycle
// ---------------------------------------------------------------------------

/**
 * Configuration for a single container.
 */
export interface ContainerSpec {
  /** Container image (required) */
  image: string;
  /** Container name (optional) */
  name?: string;
  /** Port mappings (e.g., "8080:80") */
  ports?: string[];
  /** Volume mounts (e.g., "/host/path:/container/path:ro") */
  volumes?: string[];
  /** Environment variables */
  env?: Record<string, string>;
  /** Command to run (overrides image CMD) */
  cmd?: string[];
  /** Entrypoint (overrides image ENTRYPOINT) */
  entrypoint?: string[];
  /** Network to attach to */
  network?: string;
  /** Remove container on exit */
  rm?: boolean;
  /** Container labels */
  labels?: Record<string, string>;
  /** Mount the root filesystem read-only */
  read_only?: boolean;
  /** Privileged mode — use sparingly (dropped with a warning on
   *  apple/container, which has no privileged concept) */
  privileged?: boolean;
  /** UID, username, or `"UID:GID"` the container's processes run as */
  user?: string;
  /** Working directory inside the container */
  workdir?: string;
  /** Linux capabilities to add (e.g. `["NET_BIND_SERVICE"]`) */
  cap_add?: string[];
  /** Linux capabilities to drop (e.g. `["ALL"]`) */
  cap_drop?: string[];
  /** Seccomp profile: a path to a JSON profile file, or `"default"`
   *  for the runtime's default profile. Emitted as
   *  `--security-opt seccomp=<value>`. Dropped with a warning on
   *  backends without seccomp support (apple/container). */
  seccomp?: string;
  /** Set `--security-opt no-new-privileges` so SUID/SGID binaries
   *  inside the container can't gain privileges via execve. Dropped
   *  with a warning on apple/container. */
  no_new_privileges?: boolean;
}

/**
 * Handle to a container instance.
 */
export interface ContainerHandle {
  /** Container ID */
  id: string;
  /** Container name (if specified) */
  name?: string;
}

/**
 * Run a container from the given spec.
 * @param spec Container configuration
 * @returns Promise resolving to ContainerHandle
 */
export function run(spec: ContainerSpec): Promise<ContainerHandle>;

/**
 * Create a container from the given spec without starting it.
 * @param spec Container configuration
 * @returns Promise resolving to ContainerHandle
 */
export function create(spec: ContainerSpec): Promise<ContainerHandle>;

/**
 * Start a previously created container.
 * @param id Container ID or name
 * @returns Promise resolving when container is started
 */
export function start(id: string): Promise<void>;

/**
 * Stop a running container.
 * @param id Container ID or name
 * @param timeout Timeout in seconds before force-terminating (default: 10)
 * @returns Promise resolving when container is stopped
 */
export function stop(id: string, timeout?: number): Promise<void>;

/**
 * Remove a container.
 * @param id Container ID or name
 * @param force If true, stop and remove a running container
 * @returns Promise resolving when container is removed
 */
export function remove(id: string, force?: boolean): Promise<void>;

// ---------------------------------------------------------------------------
// Container Inspection and Listing
// ---------------------------------------------------------------------------

/**
 * Information about a container.
 */
export interface ContainerInfo {
  /** Container ID */
  id: string;
  /** Container name */
  name: string;
  /** Image reference */
  image: string;
  /** Container status (e.g., "running", "exited") */
  status: string;
  /** Port mappings */
  ports: string[];
  /** Creation timestamp (ISO 8601) */
  created: string;
}

/**
 * List containers.
 *
 * @param all If true, include stopped containers
 * @returns Promise resolving to a **JSON-encoded** `ContainerInfo[]`
 *   string — call `JSON.parse(await list(all))` to recover the array.
 *   The string-shape return reflects Perry's current FFI contract;
 *   server-side array-materialization is a planned ergonomics task.
 */
export function list(all?: boolean): Promise<string>;

/**
 * Inspect a container.
 *
 * @param id Container ID or name
 * @returns Promise resolving to a **JSON-encoded** `ContainerInfo`
 *   string. Call `JSON.parse(await inspect(id))` to recover the object.
 */
export function inspect(id: string): Promise<string>;

// ---------------------------------------------------------------------------
// Container Logs and Exec
// ---------------------------------------------------------------------------

/**
 * Logs captured from a container.
 */
export interface ContainerLogs {
  /** Standard output */
  stdout: string;
  /** Standard error */
  stderr: string;
}

/**
 * Get logs from a container.
 *
 * @param id Container ID or name
 * @param options Options for logs (`tail`: number of trailing lines)
 * @returns Promise resolving to a **JSON-encoded** `ContainerLogs`
 *   string. Call `JSON.parse(await logs(id))` to recover
 *   `{ stdout, stderr }`.
 */
export function logs(
  id: string,
  options?: {
    /** Number of lines to return from the end (negative = no limit) */
    tail?: number;
  }
): Promise<string>;

/**
 * Execute a command in a running container.
 *
 * @param id Container ID or name
 * @param cmd Command to execute
 * @param options Options for exec
 * @returns Promise resolving to a **JSON-encoded** `ContainerLogs`
 *   string. Call `JSON.parse(await exec(id, cmd))` to recover
 *   `{ stdout, stderr }`.
 */
export function exec(
  id: string,
  cmd: string[],
  options?: {
    /** Environment variables */
    env?: Record<string, string>;
    /** Working directory */
    workdir?: string;
  }
): Promise<string>;

// ---------------------------------------------------------------------------
// Image Management
// ---------------------------------------------------------------------------

/**
 * Information about a container image.
 */
export interface ImageInfo {
  /** Image ID */
  id: string;
  /** Repository name */
  repository: string;
  /** Image tag */
  tag: string;
  /** Image size in bytes */
  size: number;
  /** Creation timestamp (ISO 8601) */
  created: string;
}

/**
 * Pull a container image from a registry.
 * @param reference Image reference (e.g., "alpine:latest", "cgr.dev/chainguard/alpine-base@sha256:...")
 * @returns Promise resolving when image is pulled
 */
export function pullImage(reference: string): Promise<void>;

/**
 * List images in the local cache.
 *
 * @returns Promise resolving to a **JSON-encoded** `ImageInfo[]` string.
 *   Call `JSON.parse(await listImages())` to recover the array.
 */
export function listImages(): Promise<string>;

/**
 * Remove an image from the local cache.
 * @param reference Image reference
 * @param force If true, remove even if image is in use
 * @returns Promise resolving when image is removed
 */
export function removeImage(reference: string, force?: boolean): Promise<void>;

// ---------------------------------------------------------------------------
// Compose (Multi-Container Orchestration)
// ---------------------------------------------------------------------------

/**
 * Multi-container application specification.
 */
export interface ComposeSpec {
  /** Compose project name — labels every container
   *  (`perry.compose.project=<name>`) and namespaces non-external
   *  volumes / networks as `<name>_<declared-name>`. Defaults to
   *  `"perry-stack"` when omitted. */
  name?: string;
  /** Compose file version */
  version?: string;
  /** Service definitions */
  services: Record<string, ComposeService>;
  /** Network definitions */
  networks?: Record<string, ComposeNetwork>;
  /** Volume definitions */
  volumes?: Record<string, ComposeVolume>;
}

/**
 * Service definition in Compose.
 *
 * Mirrors `perry/compose`'s `Service` interface — kept in sync via the
 * `types_compose_service_keys_in_sync` invariant test in
 * `crates/perry-stdlib/tests/container_workspace_invariants.rs`.
 */
export interface ComposeService {
  /** Container image */
  image?: string;
  /** Explicit container name (required for cross-service DNS today — see
   *  `docs/src/container/networking.md#cross-service-dns`) */
  container_name?: string;
  /** Build configuration */
  build?: {
    /** Build context directory */
    context?: string;
    /** Containerfile path (relative to context) */
    dockerfile?: string;
    /** Build-time arguments */
    args?: Record<string, string>;
    /** Labels to add to the built image */
    labels?: Record<string, string>;
    /** Build target stage */
    target?: string;
    /** Network to use during build */
    network?: string;
  };
  /** Command to run */
  command?: string | string[];
  /** Override container entrypoint */
  entrypoint?: string | string[];
  /** Environment variables */
  environment?: Record<string, string> | string[];
  /** Container labels */
  labels?: Record<string, string>;
  /** Port mappings, e.g. `"8080:80"` */
  ports?: string[];
  /** Volume mounts: named (`"forgejo-data:/data"`) or bind
   *  (`"./config:/app/config:ro"`) */
  volumes?: string[];
  /** Networks to attach to */
  networks?: string[];
  /** Service dependencies — array form OR map form with conditions
   *  (`{ db: { condition: "service_healthy" } }`) */
  depends_on?: string[] | Record<string, { condition?: string }>;
  /** Restart policy */
  restart?: "no" | "always" | "on-failure" | "unless-stopped";
  /** Healthcheck configuration */
  healthcheck?: ComposeHealthcheck;
  /** UID / username the container's processes run as (`"1000"` / `"git"`) */
  user?: string;
  /** Working directory inside the container */
  working_dir?: string;
  /** Read-only root filesystem */
  read_only?: boolean;
  /** Privileged mode — use sparingly */
  privileged?: boolean;
  /** Linux capabilities to add (e.g. `["NET_ADMIN"]`) */
  cap_add?: string[];
  /** Linux capabilities to drop (e.g. `["ALL"]`) */
  cap_drop?: string[];
  /** Security options (compose-spec § service.security_opt), e.g.
   *  `["seccomp=default", "no-new-privileges"]`. Parsed into the
   *  engine's `SecurityProfile`; entries a backend can't honor are
   *  dropped with a warning. */
  security_opt?: string[];
}

/**
 * Healthcheck configuration (compose-spec § service.healthcheck).
 *
 * `interval`, `timeout`, `start_period` accept Go-duration strings
 * (`"30s"`, `"2m"`, `"1h30m"`); the OCI runtime parses them.
 *
 * `test` is either a `["NONE"]` sentinel that disables the image's own
 * healthcheck, or `["CMD", "<cmd>", "<arg>", ...]` /
 * `["CMD-SHELL", "<shell-line>"]`.
 */
export interface ComposeHealthcheck {
  /** Test command (string or array) */
  test: string | string[];
  /** Check interval (e.g., `"30s"`) */
  interval?: string;
  /** Timeout (e.g., `"10s"`) */
  timeout?: string;
  /** Number of retries before unhealthy */
  retries?: number;
  /** Startup grace period (e.g., `"40s"`) */
  start_period?: string;
  /** Disable the image's built-in healthcheck */
  disable?: boolean;
}

/**
 * Network configuration.
 */
export interface ComposeNetwork {
  /** Network driver (`"bridge"` is the default; `"overlay"` for swarm) */
  driver?: string;
  /** External: don't create — assume the network already exists */
  external?: boolean;
  /** Override the network's runtime name */
  name?: string;
  /**
   * Internal-only network: containers attached can only reach other
   * containers on the same network — no external bridge / routing,
   * no host-network egress. Use this for the database side of a
   * web/db split so postgres etc. can't be reached from the host.
   */
  internal?: boolean;
  /** Driver-specific options */
  driver_opts?: Record<string, string>;
  /** Labels */
  labels?: Record<string, string>;
}

/**
 * Volume configuration.
 */
export interface ComposeVolume {
  /** Volume driver */
  driver?: string;
  /** External: don't create — assume the volume already exists */
  external?: boolean;
  /** Override the volume's runtime name */
  name?: string;
  /** Driver-specific options */
  driver_opts?: Record<string, string>;
  /** Labels */
  labels?: Record<string, string>;
}

/**
 * Bring up a Compose stack.
 * @param spec Compose specification
 * @returns Promise resolving to the stack ID (number)
 */
export function composeUp(spec: ComposeSpec): Promise<number>;

// ---------------------------------------------------------------------------
// Cleanup / teardown helpers (no ComposeHandle required)
// ---------------------------------------------------------------------------

/**
 * Summary returned by `downByProject` / `downAll`. JSON-encoded across
 * the FFI boundary — call `JSON.parse(await downByProject(...))` to
 * get this typed shape.
 */
export interface CleanupReport {
  containers_removed: number;
  networks_removed: number;
  volumes_removed: number;
  /** Per-resource error messages; cleanup is best-effort */
  errors: string[];
}

/**
 * Options for `downByProject` / `downAll`.
 */
export interface CleanupOptions {
  /** Drop named volumes (default false — preserves data). */
  volumes?: boolean;
  /** Best-effort prune unused networks (default true). */
  networks?: boolean;
}

/**
 * Tear down every container labelled with `perry.compose.project =
 * <project>`, regardless of whether you still hold the original
 * `ComposeHandle`. Useful when:
 *
 *   - The original process crashed without calling `down()`.
 *   - You're in a different process / session and don't have the
 *     in-memory handle anymore.
 *   - You're cleaning up between dev iterations.
 *
 * @returns Promise resolving to a JSON-encoded `CleanupReport` string.
 *   Call `JSON.parse(await downByProject('myapp'))` to parse it.
 */
export function downByProject(
  project: string,
  options?: CleanupOptions,
): Promise<string>;

/**
 * Tear down EVERY Perry-managed container on this host. **Use
 * sparingly** — this stops every stack the user has ever brought up
 * via `perry/compose`, regardless of which terminal session it's
 * running in. Returns the same JSON-encoded `CleanupReport` shape as
 * `downByProject`.
 */
export function downAll(options?: CleanupOptions): Promise<string>;

/**
 * Idempotent single-container removal. Stop + force-remove if the
 * container exists; treat NotFound as success. Returns `"true"` if
 * the container was found and removed, `"false"` if it didn't exist.
 *
 * Useful in test cleanup paths and recovery scripts where you're not
 * sure whether a container was ever started.
 */
export function removeIfExists(
  idOrName: string,
  force?: boolean,
): Promise<string>;

// ---------------------------------------------------------------------------
// Platform Information
// ---------------------------------------------------------------------------

/**
 * Get the name of the container backend being used.
 * @returns "apple/container" on macOS/iOS, "podman" on all other platforms
 */
export function getBackend(): string;

/**
 * Detected container runtime metadata. Returned by `detectBackend()` after
 * `JSON.parse`'ing the result.
 */
export interface BackendInfo {
  /** Canonical backend name (e.g. `"docker"`, `"podman"`, `"apple/container"`) */
  name: string;
  /** Whether the backend was successfully probed and is ready to use */
  available: boolean;
  /** Failure reason if `available === false` (empty string when available) */
  reason: string;
  /** Optional CLI version string when the backend is available */
  version?: string;
}

/**
 * Probe for available container runtimes and return details about each.
 *
 * @returns Promise resolving to a **JSON-encoded** `BackendInfo[]`
 *   string. Call `JSON.parse(await detectBackend())` to recover the
 *   typed array. Each entry includes `name`, `available`, `reason`
 *   (failure reason if any), and an optional `version` field.
 *   Example:
 *
 *   ```ts
 *   const probed = JSON.parse(await detectBackend()) as BackendInfo[];
 *   const live   = probed.filter(b => b.available);
 *   ```
 */
export function detectBackend(): Promise<string>;

/**
 * Probe **every** backend in the platform priority list and return
 * a JSON-encoded `BackendInfo[]` — one entry per candidate, in
 * priority order, regardless of whether any are actually installed.
 *
 * Distinct from `detectBackend()`, which short-circuits on the first
 * success and only tells you the *winner* (or, on no-match, the full
 * failure list). `getAvailableBackends()` always probes the full list
 * so you can see which subset is reachable.
 *
 * Use this for:
 * - Diagnostics ("what's installed on this host?")
 * - CI matrix lane resolution ("can I run the apple/container lane here?")
 * - User-facing backend pickers
 * - Programmatic fallback chains: take the available subset and feed
 *   it to `setBackends()` for an order-preserving pin.
 *
 * Each candidate gets a 2-second probe timeout. Worst case is
 * `2s × len(getBackendPriority())` (≤16s on macOS, ≤6s on Linux);
 * in practice most candidates fail fast (`which` miss).
 *
 * @returns JSON-encoded `BackendInfo[]`, length always equal to
 *   `getBackendPriority().length`. Order matches the priority list.
 *
 * @example
 *   import { getAvailableBackends, setBackends, BackendInfo } from 'perry/container';
 *
 *   const all = JSON.parse(await getAvailableBackends()) as BackendInfo[];
 *   const ready = all.filter(b => b.available);
 *   if (ready.length === 0) {
 *     throw new Error('no container runtime installed on this host');
 *   }
 *   // Feed the available subset to setBackends in priority order.
 *   await setBackends(ready.map(b => b.name));
 *
 * @example
 *   // CI lane gating — skip a test job if its required backend isn't here.
 *   const all = JSON.parse(await getAvailableBackends()) as BackendInfo[];
 *   const apple = all.find(b => b.name === 'apple/container');
 *   if (!apple?.available) {
 *     console.log(`skip: apple/container not available — ${apple?.reason}`);
 *     process.exit(0);
 *   }
 */
export function getAvailableBackends(): Promise<string>;

/**
 * Pin a specific container backend programmatically. Equivalent to
 * setting `PERRY_CONTAINER_BACKEND=<name>` before process start, but
 * callable from TS. **Must be called before any other container op**
 * — the global backend singleton is initialised lazily on first use,
 * and `setBackend()` rejects after that point (the `OnceLock`-based
 * cache can't be reset, so a mid-process switch would silently fail).
 *
 * Valid names come from `getBackendPriority()`. Common values:
 * `"apple/container"`, `"podman"`, `"docker"`, `"orbstack"`,
 * `"colima"`, `"rancher-desktop"`, `"lima"`, `"nerdctl"`.
 *
 * @returns Promise resolving to the canonical backend name on success;
 *   rejects with one of:
 *   - `"backend already initialised; setBackend must be called before any other container op"`
 *   - `"unknown backend: '<name>'. Valid: [...]"`
 *   - `"backend probe failed: <reason>"`
 *
 * @example
 *   import { setBackend, up } from 'perry/container';
 *   // Pin docker explicitly, override platform default (apple/container on macOS).
 *   await setBackend('docker');
 *   await up({ services: { web: { image: 'nginx' } } });
 */
export function setBackend(name: string): Promise<string>;

/**
 * User-defined priority list — try each backend in order, first
 * available wins. Generalises `setBackend(name)` for the common
 * pattern "prefer podman, fall back to docker."
 *
 * Equivalent to `PERRY_CONTAINER_BACKEND=name1,name2,...` before
 * process start. Must be called before any other container op (the
 * global backend `OnceLock` can't be reset, same contract as
 * `setBackend()`).
 *
 * Each name must come from `getBackendPriority()`. Validation happens
 * BEFORE the env var is set, so a typo doesn't half-commit. The
 * promise resolves with the canonical name of the backend that
 * actually got picked.
 *
 * @param names  Non-empty array of backend names in user-preferred
 *   order. Empty array → reject with `"setBackends requires a
 *   non-empty array"`.
 *
 * @returns Promise resolving to the picked backend's canonical name,
 *   or rejecting with one of:
 *   - `"backend already initialised; setBackends must be called before any other container op"`
 *   - `"setBackends requires a non-empty array"`
 *   - `"unknown backend: '<typo>'. Valid: [...]"`
 *   - `"none of the requested backends could be probed: ..."`
 *
 * @example
 *   import { setBackends, up } from 'perry/container';
 *   // Try podman first (rootless, OCI-compatible), fall back to docker.
 *   const picked = await setBackends(['podman', 'docker']);
 *   console.log('using', picked);
 *   await up({ services: { ... } });
 *
 * @example
 *   // CI matrix: each lane pins a different priority list.
 *   //   lane "rootless"  → ['podman', 'nerdctl']
 *   //   lane "macos-vm"  → ['apple/container', 'colima']
 *   //   lane "fallback"  → ['docker']
 */
export function setBackends(names: string[]): Promise<string>;

/**
 * Returns the platform-specific backend probe order as a JSON-encoded
 * `string[]`. Useful for diagnostics + validating an argument to
 * `setBackend()`.
 *
 * The ordering encodes three priorities in descending precedence:
 *
 * 1. **Platform-native first** — `apple/container` is the very first
 *    probe on macOS/iOS.
 * 2. **OCI-compatible / rootless before daemon-based** — `podman`
 *    (rootless, daemonless, OCI-compatible) ranks ahead of `docker`
 *    on every platform; `nerdctl` (containerd-native) sits between.
 * 3. **Docker is always the fallback** — never preferred, never first.
 *
 * Override per-process via `PERRY_CONTAINER_BACKEND=<name>` env var
 * (or the `setBackend()` runtime API above).
 *
 * @returns JSON-encoded `string[]` of backend names in probe order.
 *   Example on macOS:
 *   `'["apple/container","orbstack","colima","rancher-desktop","lima","podman","nerdctl","docker"]'`
 */
export function getBackendPriority(): string;

/**
 * Strictness modes for `selectBackendFor()`.
 *
 * - `"strict-native"` — only natively-supported features count. A
 *   spec needing `privileged: true` rules out apple/container even
 *   though apple emulates restart policies host-side.
 * - `"accept-emulated"` (default) — engine-emulated features count
 *   as a degraded but functional substitute. Apple's host-side
 *   restart loop, healthcheck polling, sigstore verification all
 *   accepted.
 * - `"accept-partial"` — also accept `Partial(reason)` support
 *   axes (e.g., apple's user-defined-bridge requires
 *   `container system start`). Suitable for dev / "just make it
 *   run" workflows.
 */
export type SelectMode = "strict-native" | "accept-emulated" | "accept-partial";

/**
 * Pick the highest-priority backend whose declared capabilities can
 * honor every feature the spec uses. Pure introspection — no probes,
 * no daemon checks, no filesystem access.
 *
 * Returns the JSON-encoded backend name (e.g. `'"apple/container"'`,
 * `'"docker"'`, `'"podman"'`) or the JSON sentinel `"null"` if no
 * backend can honor the spec under the given strictness mode.
 *
 * @example
 *   import { selectBackendFor, setBackend, up } from 'perry/container';
 *
 *   const spec = {
 *     services: {
 *       db: { image: 'postgres:16', privileged: true },
 *     },
 *   };
 *
 *   // privileged: true rules out apple/container — picks docker.
 *   const best = JSON.parse(selectBackendFor(JSON.stringify(spec)));
 *   // => "docker"
 *   await setBackend(best);
 *   await up(spec);
 *
 * @param spec  JSON-encoded ComposeSpec
 * @param mode  Strictness — defaults to `"accept-emulated"`
 * @returns     JSON-encoded backend name or `"null"`
 */
export function selectBackendFor(spec: string, mode?: SelectMode): string;
