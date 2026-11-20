// Regression test for issue #154: ES2024 explicit-resource-management
// `using` / `await using` declarations with `[Symbol.dispose]()` /
// `[Symbol.asyncDispose]()` hooks. Output is byte-for-byte compared
// against `node --experimental-strip-types`.
//
// Covers:
//   - Sync `using` calling `[Symbol.dispose]()`
//   - `await using` calling `await [Symbol.asyncDispose]()`
//   - Reverse-order disposal
//   - Multi-binding `using a = e1, b = e2, c = e3` (rightmost disposes first)
//   - Skipping disposal when the binding is null
//
// Out of scope (separate, broader limitation):
//   - SuppressedError chaining when the body throws and a disposer also
//     throws — Perry's try/finally without a catch clause doesn't currently
//     re-propagate (separate, pre-existing limitation).

const log: string[] = [];

class Resource {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  [Symbol.dispose](): void {
    log.push("sync-dispose:" + this.name);
  }
}

class AsyncResource {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  async [Symbol.asyncDispose](): Promise<void> {
    await new Promise<void>((r) => setTimeout(r, 1));
    log.push("async-dispose:" + this.name);
  }
}

function syncCase(): void {
  using r1 = new Resource("a");
  using r2 = new Resource("b");
  log.push("sync-body");
  // r2 disposed first, then r1
}

function multiBindingCase(): void {
  using x = new Resource("x"), y = new Resource("y"), z = new Resource("z");
  log.push("multi-body");
  // Disposed z, y, x — rightmost first
}

function nullSkipCase(): void {
  using r1: Resource | null = null;
  using r2 = new Resource("after-null");
  log.push("null-body");
  // Only r2 disposes; r1 is null and is skipped
}

async function asyncCase(): Promise<void> {
  await using r1 = new AsyncResource("p");
  await using r2 = new AsyncResource("q");
  log.push("async-body");
  // r2 disposed first, then r1
}

async function main(): Promise<void> {
  syncCase();
  multiBindingCase();
  nullSkipCase();
  await asyncCase();
  console.log(log.join("\n"));
}

main();
