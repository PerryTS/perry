// Pinned Node 26.5.1 parity for the non-worker half of read_holder_accessor.rs.
// Expected: 2000 2000 8000 31000 42000 31
function makeHolder() {
  return class { get path(): number {
    if ((globalThis as any).accessorCollect) {
      (globalThis as any).accessorCollect = false;
      (globalThis as any).gc();
    }
    return (this as any).child.value + (this as any).factor;
  } };
}
const HolderA = makeHolder();
const HolderB = makeHolder();
class Host { n = 1; child = { value: 1 }; }
function read(o: any): number { return o.path; }
async function main(): Promise<void> {
  const o: any = new Host();
  const a: any = HolderA.prototype;
  const b: any = HolderB.prototype;
  a.factor = 1;
  b.factor = 7;
  // An explicit, serial prototype link gives Host a MIXED identity.
  Object.setPrototypeOf(o, a);
  let first = 0;
  for (let i = 0; i < 1000; i++) {
    if (i === 500) (globalThis as any).accessorCollect = true;
    first += read(o);
  }
  let keep: any[] = [];
  for (let i = 0; i < 20000; i++) keep.push({ i });
  (globalThis as any).gc();
  keep = [];
  let moved = 0;
  for (let i = 0; i < 1000; i++) moved += read(o);
  // Both prototypes declare the same accessor key. A receiver-link check,
  // not holder shape alone, must reject a stale answer from A.
  Object.setPrototypeOf(o, b);
  let replaced = 0;
  for (let i = 0; i < 1000; i++) replaced += read(o);
  Object.defineProperty(b, 'path', {
    configurable: true,
    get() {
      if ((globalThis as any).accessorCollect) {
        (globalThis as any).accessorCollect = false;
        (globalThis as any).gc();
      }
      return (this as any).child.value + 30;
    }
  });
  let mutated = 0;
  for (let i = 0; i < 1000; i++) mutated += read(o);
  // A declared class with no user override is the bare CLASS identity that
  // resolves through the class registry, as Zod's ParseInputLazyPath does.
  class Bare { n = 2; get path(): number { return this.n + 40; } }
  function bareRead(x: any): number { return x.path; }
  const bare: any = new Bare();
  let declared = 0;
  for (let i = 0; i < 1000; i++) declared += bareRead(bare);
  console.log(first, moved, replaced, mutated, declared, read(o));
}
main();
