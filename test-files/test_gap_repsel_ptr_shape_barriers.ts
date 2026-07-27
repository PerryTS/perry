// Representation-selection Phase 3b: §5.2 soundness barriers.
// Every construct here DISQUALIFIES Ptr<Shape> promotion (module-wide under
// the first-increment conservative rule); the observable behavior must remain
// byte-exact vs Node — the guarded/boxed path handles the dynamic shape ops.

class Cfg {
  host: string;
  port: number;
  constructor(host: string, port: number) {
    this.host = host;
    this.port = port;
  }
  url(): string {
    return this.host + ":" + this.port;
  }
}

// 1. Object.defineProperty converts a data field into an accessor — the read
//    AFTER it must observe the getter, not a stale fixed-offset slot.
function defineProp(): string {
  const c = new Cfg("localhost", 8080);
  let acc = 0;
  for (let i = 0; i < 20; i++) acc += c.port;
  Object.defineProperty(c, "port", {
    get() {
      return 9999;
    },
  });
  return acc + ":" + c.port + ":" + c.url();
}
console.log(defineProp());

// 2. delete removes an own field — reads fall through to undefined.
function deleteField(): string {
  const c: any = new Cfg("a", 1);
  let acc = 0;
  for (let i = 0; i < 10; i++) acc += c.port;
  delete c.port;
  return acc + ":" + String(c.port) + ":" + ("port" in c);
}
console.log(deleteField());

// 3. setPrototypeOf swaps method resolution mid-function.
function protoSwap(): string {
  const c = new Cfg("h", 2);
  const before = c.url();
  Object.setPrototypeOf(c, {
    url() {
      return "swapped";
    },
  });
  return before + "->" + c.url();
}
console.log(protoSwap());

// 4. __proto__ write on an anon-shape record.
function protoWrite(): string {
  const rec = { key: "k", value: 1 };
  const before = rec.value;
  (rec as any).__proto__ = {
    get bonus() {
      return 41;
    },
  };
  return before + ":" + (rec as any).bonus;
}
console.log(protoWrite());

// 5. Reflect.defineProperty (mutating Reflect) on a builder object.
function reflectDefine(): string {
  const b: any = {};
  b.a = 1;
  Reflect.defineProperty(b, "a", { value: 77, writable: false });
  let threw = "no";
  try {
    b.a = 100; // strict mode: assigning a non-writable data property throws
  } catch (e) {
    threw = e instanceof TypeError ? "TypeError" : "other";
  }
  return b.a + ":" + threw + ":" + JSON.stringify(b);
}
console.log(reflectDefine());

// 6. Alias that escapes through a container: the object is reachable from
//    outside, so shape mutation through the alias must be observed.
const registry: any[] = [];
function aliasEscape(): string {
  const c = new Cfg("x", 3);
  registry.push(c);
  let acc = 0;
  for (let i = 0; i < 10; i++) acc += c.port;
  mutateRegistry();
  return acc + ":" + c.port;
}
function mutateRegistry(): void {
  for (const o of registry) {
    Object.defineProperty(o, "port", {
      get() {
        return -1;
      },
    });
  }
}
console.log(aliasEscape());
