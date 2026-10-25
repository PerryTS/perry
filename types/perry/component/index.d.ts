/** Limits apply independently to each component instance. No filesystem,
 * environment, stdio or network capability is granted implicitly. */
export interface ComponentOptions {
  /** Maximum total linear-memory bytes per component. Default: 64 MiB. */
  memoryBytes?: number;
  /** Instruction budget for initialization and each call. Default: 10,000,000. */
  fuel?: number;
  /** Maximum core instances, memories and tables. Default: 64. */
  instances?: number;
  /** Enable Preview 2 providers. Omission links no WASI providers. */
  wasi?: WasiOptions;
}

declare const componentBrand: unique symbol;
declare const resourceBrand: unique symbol;
export interface ComponentHandle { readonly [componentBrand]: true; }
export interface BorrowedResource<T extends string> {
  readonly [resourceBrand]: T;
}
export interface OwnedResource<T extends string> extends BorrowedResource<T> {
  /** A borrow remains valid only while its owner and current call are live. */
  borrow(): BorrowedResource<T>;
  /** Releases the resource exactly once; transferred owners cannot be reused. */
  dispose(): void;
}

/** Use generated WIT bindings for application calls. These lower-level entry
 * points validate the manifest and component's canonical types at load time. */
export function loadComponent(bytes: Uint8Array, contract: string, imports: unknown, options?: ComponentOptions): ComponentHandle;
export function callComponent(handle: ComponentHandle, interfaceName: string, functionName: string, args: unknown[]): unknown;
export function disposeComponent(handle: ComponentHandle): void;

declare const httpServerBrand: unique symbol;
export interface ComponentHttpServer {
  readonly [httpServerBrand]: true;
  /** Bound numeric address, including the selected port when binding port 0. */
  readonly address: string;
}
export interface ComponentHttpServerOptions {
  /** Explicit numeric socket address, e.g. "127.0.0.1:0" or "[::1]:8080". */
  address: string;
  /** Positive connection limit. Default: 64. */
  maxConnections?: number;
  handshakeTimeoutMs?: number;
  requestHeadTimeoutMs?: number;
  responseHeadTimeoutMs?: number;
  betweenBytesTimeoutMs?: number;
  lingerTimeoutMs?: number;
  /** Explicit PEM credentials; no ambient certificate or key files are read. */
  tls?: { certificatePem: string; privateKeyPem: string };
}
/** Native-only HTTP/1.1 listener. Each accepted connection serves one request.
 * Creating a listener does not grant a component outbound network access. */
export function createComponentHttpServer(options: ComponentHttpServerOptions): ComponentHttpServer;
/** Accept one request and invoke the selected P2 incoming-handler or P3 0.3.0 handler.
 * Prefer generated Component.handleHttp(server), which supplies its WIT version.
 * Resolves after the response transport completes; traps, cancellation and
 * transport errors reject. Bodies stream through bounded native buffers.
 * Pending work keeps the event loop alive. The P2 guest handler executes
 * synchronously on the owning JS thread. P3 suspends canonical tasks and async
 * imports without blocking that thread. Closing the listener or disposing the
 * component cancels pending P3 handling. Fuel does not bound host I/O waits. */
export function handleComponentHttpRequest(server: ComponentHttpServer, handle: ComponentHandle, interfaceName: string): Promise<void>;
/** Idempotently stop the listener and cancel pending accepts and exchanges. */
export function closeComponentHttpServer(server: ComponentHttpServer): void;

/** Execute a WASI P2 or 0.3.0 command once in an isolated native store, then release it.
 * Returns 0/1 for wasi:cli/run's ok/err and preserves explicit WASI exit codes.
 * Guest exit never terminates Perry. Invalid components, traps and exhausted
 * limits throw ERR_COMPONENT. No WIT bindings or application imports are needed.
 * Omitting options.wasi enables providers with the grants of wasi: {}.
 * Fuel bounds guest instructions, not time spent in blocking host I/O. */
export function runComponentCommand(bytes: Uint8Array, options?: ComponentOptions): number;
/** Registers the exports of a Perry-authored library component. */
export function registerComponentExports(contract: string, exports: unknown): void;
export function callComponentImport(interfaceName: string, functionName: string, args: unknown[]): unknown;

/** Explicit per-instance grants. An empty object enables clocks and randomness;
 * stdin is closed, stdout/stderr are sinks, and files/network/env/args are denied. */
export interface WasiOptions {
  args?: string[];
  env?: Record<string, string>;
  inheritStdin?: boolean;
  inheritStdout?: boolean;
  inheritStderr?: boolean;
  /** Directory grants default to read-only. Symlinks cannot escape the grant. */
  preopens?: { hostPath: string; guestPath: string; readOnly?: boolean }[];
  /** Protocols default off; addresses default empty. DNS grants name resolution,
   * not access to the resolved addresses. Grants apply to binds and connects. */
  network?: {
    tcp?: boolean;
    udp?: boolean;
    dns?: boolean;
    /** IP literals only. Omitted port explicitly permits every port on that IP. */
    addresses?: { address: string; port?: number }[];
  };
}

/** Guest-only resource operations. Prefer the generated WIT resource factories.
 * The type identity is generated from the component's WIT world. */
export function createComponentResource<T extends string>(type: number, payload: unknown, destructor?: (payload: unknown) => void): OwnedResource<T>;
export function getComponentResourceValue<T extends string>(type: number, resource: BorrowedResource<T>): unknown;

/** Native host resource operations. Generated load() builders supply factories
 * bound to the component instance and canonical WIT resource type. */
export function createComponentHostResource<T extends string>(handle: ComponentHandle, type: string, payload: unknown, destructor?: (payload: unknown) => void): OwnedResource<T>;
export function getComponentHostResourceValue<T extends string>(handle: ComponentHandle, type: string, resource: BorrowedResource<T>): unknown;
