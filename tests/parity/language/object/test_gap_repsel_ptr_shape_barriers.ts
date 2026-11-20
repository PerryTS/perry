// Representation-selection Phase 3b: §5.2 soundness barriers.
// Every construct here DISQUALIFIES Ptr<Shape> promotion (module-wide under
// the first-increment conservative rule — any defineProperty / delete /
// setPrototypeOf / Proxy / mutating-Reflect site disables promotion for the
// whole module); the observable behavior must remain byte-exact vs Node —
// the guarded/boxed paths handle the dynamic shape ops.
//
// (Lone `__proto__` writes and strict-mode throw-on-non-writable assignment
// are pre-existing categorical gaps unrelated to this phase and are not
// exercised here.)

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

// 3. setPrototypeOf swaps the prototype — a data-property lookup through the
//    NEW prototype must be observed after the swap.
function protoData(): string {
  const c = new Cfg("h", 2);
  const before = (c as any).bonus;
  Object.setPrototypeOf(c, { bonus: 42 });
  return String(before) + "->" + String((c as any).bonus) + ":" + c.host;
}
console.log(protoData());

// 4. Reflect.defineProperty (mutating Reflect) makes a builder field
//    non-writable; Reflect.set reports the rejected write without throwing.
function reflectDefine(): string {
  const b: any = {};
  b.a = 1;
  Reflect.defineProperty(b, "a", { value: 77, writable: false });
  const ok = Reflect.set(b, "a", 100);
  return b.a + ":" + ok + ":" + JSON.stringify(b);
}
console.log(reflectDefine());

// 5. Alias that escapes through a container: the object is reachable from
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
