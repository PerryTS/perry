// Differential cover for the inherited-read cache (lane 3).
//
// Every case here reads the SAME (receiver shape, key) pair repeatedly so the
// cache primes, then mutates something the cached entry depends on and reads
// again. A cache that primes but never invalidates prints the pre-mutation
// value and this file fails against node; a cache that never primes prints the
// right answers and passes — which is why the runtime suite asserts the hit
// COUNTERS as well, and why the instruction count is quoted separately.
//
// Nothing here may be hoisted by node: each loop body reads through a live
// object that the previous statement mutated.

function warm(read: () => unknown, n: number): unknown {
  let last: unknown;
  for (let i = 0; i < n; i++) {
    last = read();
  }
  return last;
}

// 1. one-level data property, then a shadowing own key, then its delete
{
  const proto: any = { a: 1 };
  const o: any = Object.create(proto);
  o.x = 0;
  console.log("1a", warm(() => o.a, 50));
  o.a = 99;
  console.log("1b", warm(() => o.a, 50));
  delete o.a;
  console.log("1c", warm(() => o.a, 50));
}

// 2. mutations of the prototype itself
{
  const proto: any = { a: 1 };
  const o: any = Object.create(proto);
  o.x = 0;
  console.log("2a", warm(() => o.a, 50));
  proto.b = 2; // key ADD on the holder: no descriptor, no epoch bump
  console.log("2b", warm(() => o.a, 50), o.b);
  proto.a = 7; // value overwrite in the cached slot
  console.log("2c", warm(() => o.a, 50));
  delete proto.a; // key REMOVE on the holder
  console.log("2d", warm(() => o.a, 50));
}

// 3. the cached key redefined as an accessor on the prototype
{
  const proto: any = { a: 1 };
  const o: any = Object.create(proto);
  o.x = 0;
  console.log("3a", warm(() => o.a, 50));
  let reads = 0;
  Object.defineProperty(proto, "a", {
    get() {
      reads++;
      return 42;
    },
    configurable: true,
  });
  console.log("3b", warm(() => o.a, 50), reads);
}

// 4. setPrototypeOf on the receiver and on an INTERIOR prototype
{
  const top: any = { a: 1 };
  const mid: any = Object.create(top);
  mid.m = 0;
  const o: any = Object.create(mid);
  o.x = 0;
  console.log("4a", warm(() => o.a, 50));
  Object.setPrototypeOf(mid, { a: "from-replacement" });
  console.log("4b", warm(() => o.a, 50));
  Object.setPrototypeOf(o, { a: "direct" });
  console.log("4c", warm(() => o.a, 50));
  // `Object.setPrototypeOf(o, null)` belongs here and is deliberately absent:
  // perry answers it from the prototype `o` was BORN with, because the generic
  // read falls back to the class registry when the recorded chain misses
  // (#10827). That is a pre-existing divergence -- `PERRY_INHERITED_IC=0`
  // prints the same wrong value -- and this cache declines the case, so
  // asserting it here would make this file red for someone else's bug. Case 8
  // covers a null prototype the receiver was born with, and 4c already proves
  // `setPrototypeOf` on the receiver invalidates an entry.
}

// 5. class instances: prototype data property, method, accessor
{
  class C {
    x = 0;
    get g() {
      return "getter";
    }
    m() {
      return "method";
    }
  }
  (C.prototype as any).d = "proto-data";
  const o: any = new C();
  console.log("5a", warm(() => o.d, 50));
  console.log("5b", warm(() => o.g, 50));
  console.log("5c", warm(() => (o.m as () => string).call(o), 50));
  (C.prototype as any).d = "changed";
  console.log("5d", warm(() => o.d, 50));
  delete (C.prototype as any).d;
  console.log("5e", warm(() => o.d, 50));
}

// 6. a Proxy in the chain must never be short-circuited
{
  let traps = 0;
  const target: any = { a: "target" };
  const proxy = new Proxy(target, {
    get(t, k, r) {
      traps++;
      return Reflect.get(t, k, r);
    },
  });
  const o: any = Object.create(proxy);
  o.x = 0;
  console.log("6a", warm(() => o.a, 10), traps === 10);
}

// 7. several receiver shapes over one prototype
{
  const proto: any = { a: "shared" };
  const receivers: any[] = [];
  for (let i = 0; i < 4; i++) {
    const r: any = Object.create(proto);
    for (let j = 0; j < i; j++) {
      r["k" + j] = j;
    }
    r.x = 0;
    receivers.push(r);
  }
  let seen = "";
  for (let i = 0; i < 40; i++) {
    const r = receivers[i & 3];
    r.x = i;
    seen = r.a;
  }
  console.log("7a", seen, receivers[3].x);
  proto.a = "replaced";
  console.log("7b", receivers.map((r) => r.a).join(","));
}

// 8. a null-prototype object never resolves inherited keys
{
  const o: any = Object.create(null);
  o.x = 0;
  console.log("8a", warm(() => o.toString, 10));
}

// 9. undefined stored on the prototype, then a real value
{
  const proto: any = { a: undefined };
  const o: any = Object.create(proto);
  o.x = 0;
  console.log("9a", warm(() => o.a, 50));
  proto.a = "now-defined";
  console.log("9b", warm(() => o.a, 50));
}
