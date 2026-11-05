// #9499: a value re-rooted after it has already been through a `gc.relocate`
// hands the consuming call a COMPLETELY DIFFERENT live object.
//
// `this._responseHandlers.set(messageId, response => { … })` in the MCP SDK
// stored the whole `jsonrpcRequest` object as the map key, so
// `_responseHandlers.has(messageId)` was false and every request timed out
// (#9485). This fixture is that shape, reduced.
//
// THE MECHANISM IS THE NATIVE-ROOT LOWERING, NOT THE ROOTING ORDER. The
// emitted IR is textbook: the key is pushed into a temp-root slot BEFORE the
// closure literal is lowered and re-read from that slot AFTER the closure's
// allocation. Only the machine code is wrong, and only under the native-root
// (RS4GC) lowering — `PERRY_RS4GC=0` is correct, and every GC knob
// (`PERRY_GC_SCAVENGE=off`, `PERRY_GC_MOVING_LOOP_POLLS=0`,
// `PERRY_GC_SCAVENGE_NURSERY_MB=1`) reproduces it identically. It is not a
// collection-timing bug at all; see `function/precise_roots.rs`'s
// `ROOT_RELOAD_LAUNDER` for the chain.
//
// TWO LOAD-BEARING REPRO PROPERTIES:
//
// 1. The key must be a value that was ALREADY temp-rooted once and then
//    released — here `this._next++`, whose read is rooted across the field
//    store. Rooting it a second time (as the `set` key) makes ONE
//    `ptr addrspace(1)` SSA value the statepoint operand at two safepoints
//    with a hole in between, which is the shape LLVM's statepoint spill-slot
//    reuse miscompiles. `const messageId = this._next` (never rooted) and
//    `set(messageId + 0, …)` (a fresh, provably-numeric key that needs no
//    root) both PASS on the unfixed compiler — they are the controls below.
// 2. The value argument must ALLOCATE, so the key is live across a safepoint.
//    `set(messageId, 7)` passes unfixed.
//
// No GC knobs, no allocation pressure, no timing: the wrong key is emitted by
// the compiler and every run of the unfixed binary prints the same thing.

function shape(m: any): string {
  return JSON.stringify(
    [...m.keys()].map((x: any) =>
      x !== null && typeof x === "object"
        ? "OBJ"
        : typeof x === "function"
          ? "FN"
          : x,
    ),
  );
}

// THE GAP. `_next` is untyped, so `messageId` is not provably a non-pointer and
// the `set` key takes a temp root; `this._next++` already rooted that same
// value across the field store.
class Gap {
  _next: any = 0;
  _handlers: any = new Map();
  run(request: any, options: any) {
    return new Promise<any>((resolve, reject) => {
      const messageId = this._next++;
      const jsonrpcRequest: any = { ...request, jsonrpc: "2.0", id: messageId };
      this._handlers.set(messageId, (response: any) => {
        if (options?.signal?.aborted) return;
        if (response instanceof Error) return reject(response);
        try {
          resolve(response);
        } catch (e) {
          reject(e);
        }
      });
      void jsonrpcRequest;
      resolve(null);
    });
  }
}

// CONTROL A: the key is read plainly, so it is never rooted twice.
class PlainKey {
  _next: any = 0;
  _handlers: any = new Map();
  run(request: any, options: any) {
    return new Promise<any>((resolve, reject) => {
      const messageId = this._next;
      const jsonrpcRequest: any = { ...request, jsonrpc: "2.0", id: messageId };
      this._handlers.set(messageId, (response: any) => {
        if (options?.signal?.aborted) return;
        if (response instanceof Error) return reject(response);
        try {
          resolve(response);
        } catch (e) {
          reject(e);
        }
      });
      void jsonrpcRequest;
      resolve(null);
    });
  }
}

// CONTROL B: `messageId + 0` is provably numeric, so `operand_protection`
// answers `Reuse` and the key pays no temp root at all. A fix that stops
// rooting keys altogether would pass the gap and leave this one meaningless,
// so it is here as the other side of the differential.
class FreshKey {
  _next: any = 0;
  _handlers: any = new Map();
  run(request: any, options: any) {
    return new Promise<any>((resolve, reject) => {
      const messageId = this._next++;
      const jsonrpcRequest: any = { ...request, jsonrpc: "2.0", id: messageId };
      this._handlers.set(messageId + 0, (response: any) => {
        if (options?.signal?.aborted) return;
        if (response instanceof Error) return reject(response);
        try {
          resolve(response);
        } catch (e) {
          reject(e);
        }
      });
      void jsonrpcRequest;
      resolve(null);
    });
  }
}

// CONTROL C: a non-allocating value leaves no safepoint in the key's window.
class NoAllocValue {
  _next: any = 0;
  _handlers: any = new Map();
  run() {
    const messageId = this._next++;
    this._handlers.set(messageId, 7);
  }
}

async function main(): Promise<void> {
  const gap = new Gap();
  await gap.run({ method: "initialize" }, {});
  console.log("gap        keys=" + shape(gap._handlers) + " has0=" + gap._handlers.has(0));

  const plain = new PlainKey();
  await plain.run({ method: "initialize" }, {});
  console.log("plain-key  keys=" + shape(plain._handlers) + " has0=" + plain._handlers.has(0));

  const fresh = new FreshKey();
  await fresh.run({ method: "initialize" }, {});
  console.log("fresh-key  keys=" + shape(fresh._handlers) + " has0=" + fresh._handlers.has(0));

  const noalloc = new NoAllocValue();
  noalloc.run();
  console.log("no-alloc   keys=" + shape(noalloc._handlers) + " has0=" + noalloc._handlers.has(0));
}

main();
