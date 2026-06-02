// Perry WebAssembly host runtime — issue #76 PoC.
//
// MVP surface — synchronous, numeric host imports only. The standard
// `Promise<{module, instance}>` async shape, host imports beyond
// numerics, `instantiateStreaming`, `Memory` / `Table` introspection,
// engine selection (wasmtime), and WASI all land as follow-ups (see the
// issue thread).
//
// **Linking is automatic.** When codegen sees any `WebAssembly.*`
// reference, the linker auto-pulls `libperry_wasm_host.a` (~1MB). The
// `--enable-wasm-runtime` flag is still accepted to force-link for
// programs that load the runtime via dlopen / FFI without a static
// reference, but the common case needs nothing.
//
// **Two call shapes work, identically.** Pick whichever reads better:
//
//   const inst = WebAssembly.instantiate(bytes);
//   inst.exports.add(2, 3);                       // standard JS shape
//   WebAssembly.callExport(inst, "add", 2, 3);    // Perry helper
//
// The standard `inst.exports.<method>(...)` shape is recognised
// syntactically — the local must be tagged at compile time as a wasm
// instance, which happens automatically when the initializer is
// `WebAssembly.instantiate(...)`. Reassigning the local through other
// expressions defeats the recognition; use `callExport` in that case.

declare global {
  namespace WebAssembly {
    interface Module {}

    /**
     * Opaque handle to an instantiated WebAssembly module. Standard
     * `instance.exports.<name>(...)` calls are recognised at compile
     * time when this type is the static type of the receiver.
     */
    interface Instance {
      readonly exports: Record<string, (...args: number[]) => number>;
    }

    interface Memory {
      readonly buffer: ArrayBuffer;
      grow(delta: number): number;
    }

    interface Table {
      readonly length: number;
      get(index: number): any;
      grow(delta: number, value?: any): number;
      set(index: number, value: any): void;
    }

    interface Global {
      value: any;
      valueOf(): any;
    }

    interface WebAssemblyErrorConstructor extends ErrorConstructor {}
  }

  const WebAssembly: {
    Module: {
      new (bytes: Uint8Array | ArrayBuffer): WebAssembly.Module;
      exports(module: WebAssembly.Module): any[];
      imports(module: WebAssembly.Module): any[];
      customSections(module: WebAssembly.Module, sectionName: string): ArrayBuffer[];
    };
    Instance: {
      new (
        module: WebAssembly.Module,
        importObject?: Record<string, any>,
      ): WebAssembly.Instance;
    };
    Memory: { new (descriptor: any): WebAssembly.Memory };
    Table: { new (descriptor: any): WebAssembly.Table };
    Global: { new (descriptor: any, value?: any): WebAssembly.Global };
    CompileError: WebAssembly.WebAssemblyErrorConstructor;
    LinkError: WebAssembly.WebAssemblyErrorConstructor;
    RuntimeError: WebAssembly.WebAssemblyErrorConstructor;
    Exception: { new (...args: any[]): any };
    Tag: { new (...args: any[]): any };
    readonly JSTag: object;

    /**
     * Present for Node feature-detection parity. Perry currently returns
     * `undefined` from this runtime stub; use `instantiate` for the supported
     * MVP execution path.
     */
    compile(bytes: Uint8Array | ArrayBuffer): Promise<WebAssembly.Module> | undefined;

    /** Quick magic + structural validation via wasmi's decoder. */
    validate(bytes: Uint8Array | ArrayBuffer): boolean;

    /**
     * Synchronously compile and instantiate a wasm module. Returns an
     * opaque instance on success, or `undefined` on failure (the
     * compile / link error is logged to stderr).
     *
     * Note: Perry MVP shape — synchronous, not the standard
     * `Promise<{module, instance}>`. Follow-up work will add the standard
     * async API surface.
     */
    instantiate(bytes: Uint8Array | ArrayBuffer): WebAssembly.Instance;

    /**
     * Present for Node feature-detection parity. Perry currently returns
     * `undefined` from streaming runtime stubs.
     */
    compileStreaming(source: any): Promise<WebAssembly.Module> | undefined;
    instantiateStreaming(source: any): Promise<any> | undefined;

    /**
     * Invoke a numeric export by name. Equivalent to
     * `instance.exports[name](...args)` but useful when the receiver
     * isn't statically typed as `WebAssembly.Instance`. Up to 4 args
     * in the current MVP.
     */
    callExport(
      instance: WebAssembly.Instance,
      name: string,
      ...args: number[]
    ): number;
  };
}

export {};
