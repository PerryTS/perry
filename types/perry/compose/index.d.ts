/**
 * perry/compose — TypeScript bindings for perry-container-compose
 *
 * Docker Compose-like experience for Apple Container, powered by Perry.
 *
 * @module perry/compose
 */

import { ContainerInfo, ContainerLogs } from "perry/container";

// ============ Configuration Types ============

/**
 * Build configuration for a service image.
 */
export interface Build {
  /** Build context directory (relative to compose file) */
  context?: string;
  /** Path to Containerfile */
  dockerfile?: string;
  /** Build-time arguments */
  args?: Record<string, string>;
  /** Labels to add to the built image */
  labels?: Record<string, string>;
  /** Build target stage */
  target?: string;
  /** Network to use during build */
  network?: string;
}

/**
 * Container healthcheck (compose-spec §service.healthcheck).
 *
 * `interval`, `timeout`, `start_period` accept Go-duration strings
 * (`"30s"`, `"2m"`, `"1h30m"`); the OCI runtime parses them.
 *
 * `test` is either a `["NONE"]` sentinel that disables the image's own
 * healthcheck, or a `["CMD", "<cmd>", "<arg>", ...]` / `["CMD-SHELL",
 * "<shell-line>"]` form.
 */
export interface Healthcheck {
  test?: string[];
  interval?: string;
  timeout?: string;
  retries?: number;
  start_period?: string;
  disable?: boolean;
}

/**
 * A single service definition in a Compose file.
 */
export interface Service {
  /** Container image reference */
  image?: string;
  /** Explicit container name */
  container_name?: string;
  /** Port mappings, e.g. "8080:80" */
  ports?: string[];
  /** Environment variables (map or KEY=VALUE list) */
  environment?: Record<string, string> | string[];
  /** Container labels */
  labels?: Record<string, string>;
  /** Volume mounts, e.g. "./data:/data:ro" */
  volumes?: string[];
  /** Build configuration */
  build?: Build;
  /** Service dependencies */
  depends_on?: string[] | Record<string, { condition?: string }>;
  /** Restart policy */
  restart?: "no" | "always" | "on-failure" | "unless-stopped";
  /** Override container entrypoint */
  entrypoint?: string | string[];
  /** Override container command */
  command?: string | string[];
  /** Networks this service is attached to */
  networks?: string[];
  /** Healthcheck (compose-spec §service.healthcheck) */
  healthcheck?: Healthcheck;
  /** UID / username the container's processes run as (`1000` / `"git"`) */
  user?: string;
  /** Working directory inside the container */
  working_dir?: string;
  /** Read-only root filesystem */
  read_only?: boolean;
  /** Privileged mode */
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
 * Network definition in a Compose file.
 */
export interface ComposeNetwork {
  driver?: string;
  external?: boolean;
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
 * Volume definition in a Compose file.
 */
export interface ComposeVolume {
  driver?: string;
  external?: boolean;
  name?: string;
}

/**
 * Root Compose file structure (docker-compose.yaml / compose.yaml).
 */
export interface ComposeSpec {
  /**
   * Compose project name. Scopes everything the engine creates:
   * containers get the `perry.compose.project=<name>` label (used by
   * `down()` and `perry/container.downByProject()`), and non-external
   * volumes / networks are namespaced `<name>_<declared-name>` so two
   * stacks declaring the same volume key don't collide and corrupt
   * each other's data. Defaults to `"perry-stack"` when omitted — set
   * it whenever more than one stack can run on the same host.
   */
  name?: string;
  version?: string;
  services: Record<string, Service>;
  networks?: Record<string, ComposeNetwork>;
  volumes?: Record<string, ComposeVolume>;
}

/**
 * Opaque handle to a running compose stack.
 */
export type ComposeHandle = number;

// ============ Options Types ============

export interface UpOptions {
  /** Start in detached mode (default: true) */
  detach?: boolean;
  /** Build images before starting */
  build?: boolean;
  /** Services to start (empty = all) */
  services?: string[];
  /** Remove orphaned containers */
  removeOrphans?: boolean;
}

export interface DownOptions {
  /** Remove named volumes */
  volumes?: boolean;
  /**
   * Remove orphaned containers: containers that still carry this
   * stack's project label (`perry.compose.project`) but whose service
   * key is no longer present in the spec — typically left behind when
   * a service was renamed or deleted between deploys. Containers
   * belonging to other projects (or without Perry's compose labels)
   * are never touched. Default false.
   */
  removeOrphans?: boolean;
}

export interface LogsOptions {
  /** Service name to get logs from (optional) */
  service?: string;
  /** Number of lines to show from the end */
  tail?: number;
}

// ============ API Functions ============

/**
 * Bring up services defined in a compose spec.
 * @param spec Compose specification object
 * @returns Promise resolving to the stack handle
 */
export function up(spec: ComposeSpec): Promise<ComposeHandle>;

/**
 * Stop and remove services in a stack.
 * @param handle Stack handle returned by up()
 * @param options Down options
 */
export function down(handle: ComposeHandle, options?: DownOptions): Promise<void>;

/**
 * List service statuses in a stack.
 *
 * @param handle Stack handle
 * @returns Promise resolving to a **JSON-encoded** `ContainerInfo[]`
 *   string. Call `JSON.parse(await ps(handle))` to recover the array.
 *   The JSON-string return shape reflects Perry's current FFI
 *   contract; server-side array-materialization is a planned
 *   ergonomics task.
 */
export function ps(handle: ComposeHandle): Promise<string>;

/**
 * Get logs from services in a stack.
 *
 * @param handle Stack handle
 * @param options Log options
 * @returns Promise resolving to a **JSON-encoded** `ContainerLogs`
 *   string. Call `JSON.parse(await logs(handle, opts))` to recover
 *   `{ stdout, stderr }`.
 */
export function logs(
  handle: ComposeHandle,
  options?: LogsOptions
): Promise<string>;

/**
 * Execute a command in a running service container within a stack.
 *
 * @param handle Stack handle
 * @param service Service name
 * @param cmd Command and arguments to execute
 * @returns Promise resolving to a **JSON-encoded** `ContainerLogs`
 *   string. Call `JSON.parse(await exec(handle, svc, cmd))` to recover
 *   `{ stdout, stderr }`.
 */
export function exec(
  handle: ComposeHandle,
  service: string,
  cmd: string[]
): Promise<string>;

/**
 * Get the resolved compose configuration.
 * @param handle Stack handle
 * @returns Validated configuration as YAML string
 */
export function config(handle: ComposeHandle): Promise<string>;

/**
 * Start existing stopped services in a stack.
 * @param handle Stack handle
 * @param services Services to start (empty = all)
 */
export function start(handle: ComposeHandle, services?: string[]): Promise<void>;

/**
 * Stop running services in a stack.
 * @param handle Stack handle
 * @param services Services to stop (empty = all)
 */
export function stop(handle: ComposeHandle, services?: string[]): Promise<void>;

/**
 * Restart services in a stack.
 * @param handle Stack handle
 * @param services Services to restart (empty = all)
 */
export function restart(handle: ComposeHandle, services?: string[]): Promise<void>;
