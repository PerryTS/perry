// Cached constructor additions must guard every admitted chain length.
function ChainWrite(this: any, n: number) { this.payload = n; }
function makeChainWrite(n: number): any { return new (ChainWrite as any)(n); }
for (let depth = 0; depth < 8; depth++) {
  let parent: any = null;
  for (let n = 0; n < depth; n++) parent = Object.create(parent);
  Object.setPrototypeOf(ChainWrite.prototype, parent);
  let sum = 0;
  for (let n = 0; n < 256; n++) sum += makeChainWrite(n).payload;
  console.log("chain", depth, sum);
  if (parent) {
    let writes = 0;
    let last = parent;
    while (Object.getPrototypeOf(last)) last = Object.getPrototypeOf(last);
    Object.defineProperty(last, "payload", {
      get() { return this.saved + 1; },
      set(v) { writes++; this.saved = v; },
      configurable: true,
    });
    const intercepted = makeChainWrite(9);
    console.log("intercept", depth, writes, intercepted.payload, Object.hasOwn(intercepted, "payload"));
    delete last.payload;
    console.log("restore", depth, makeChainWrite(10).payload);
  }
}
