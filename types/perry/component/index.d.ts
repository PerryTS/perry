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
