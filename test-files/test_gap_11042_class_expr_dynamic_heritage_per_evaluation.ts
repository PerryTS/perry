// #11042: a class expression with DYNAMIC heritage (`class extends <runtime
// value> {}`) evaluated inside a function had no per-evaluation identity
// unless it also carried statics, captures, or private elements. Every
// evaluation was the SAME shared-template class: one class id, one parent
// edge (the last evaluation's `extends` won), one prototype-method table.
// `@redis/client`'s `attachConfig` builds `Class = class extends BaseClass {}`
// once for `RedisClient` (an EventEmitter subclass) and once for its Multi
// command class, so the client lost its EventEmitter ancestry and
// `client.on("error", cb)` resolved to `events.on(emitter, name)`, throwing
// `The "emitter" argument must be an instance of EventEmitter`.
import { attachConfig } from "./gap_11042_class_expr_dynamic_heritage_helper.ts";
import * as node_events_1 from "node:events";

// ── 1. plain mixin factory reached through a function VALUE (never
//       specialized per call site) ──
class A {
  a() {
    return "A.a";
  }
}
class B {
  b() {
    return "B.b";
  }
}
function mixin(Base: any) {
  const RESP = 2,
    Class = class extends Base {};
  return Class;
}
const factories: Array<(base: any) => any> = [mixin];
for (const f of factories) {
  const X = f(A);
  const Y = f(B);
  const x = new X();
  const y = new Y();
  console.log("distinct classes:", X !== Y);
  console.log("X extends A:", Object.getPrototypeOf(X) === A, "| Y extends B:", Object.getPrototypeOf(Y) === B);
  console.log("x instanceof A:", x instanceof A, "| y instanceof B:", y instanceof B, "| x instanceof X:", x instanceof X);
  console.log("x.a():", x.a(), "| y.b():", y.b());
}

// ── 2. the redis shape: cross-module `attachConfig`, EventEmitter base
//       reached as `<namespace>.EventEmitter`, prototype-assigned commands,
//       a second evaluation BEFORE the first class is constructed ──
class RedisLike extends node_events_1.EventEmitter {
  static factory() {
    const Client = attachConfig({
      BaseClass: RedisLike,
      commands: { get: "GET", set: "SET" },
      createCommand: (cmd: string) =>
        function (this: any, ...args: any[]) {
          return `${cmd} ${args.join(" ")} via ${this.name}`;
        },
    });
    // Second evaluation of the same class expression, as redis does for
    // `Client.prototype.Multi = RedisClientMultiCommand.extend(config)`.
    Client.prototype.Multi = attachConfig({
      BaseClass: MultiLike,
      commands: { exec: "EXEC" },
      createCommand: (cmd: string) => () => cmd,
    });
    return (options: any) => Object.create(new Client(options));
  }
  name: string;
  constructor(options: any) {
    super();
    console.log("RedisLike constructor:", options.name);
    this.name = options.name;
  }
  hello() {
    return "hello from " + this.name;
  }
}
class MultiLike {
  constructor() {
    console.log("MultiLike constructor (must not run for the client)");
  }
}

const client = RedisLike.factory()({ name: "c1" });
console.log("typeof client.on:", typeof client.on);
console.log("client.hello():", client.hello());
console.log("client.get():", client.get("k"));
console.log("client.set():", client.set("k", "v"));
console.log("typeof client.exec:", typeof client.exec);
client.on("error", (err: string) => console.log("caught:", err));
console.log("emit returned:", client.emit("error", "boom"));
console.log("listenerCount:", client.listenerCount("error"));
const multi = new client.Multi();
console.log("multi.exec():", multi.exec(), "| typeof multi.get:", typeof multi.get);
