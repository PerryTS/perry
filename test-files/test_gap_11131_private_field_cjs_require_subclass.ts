// #11131: a top-level class that captures a CommonJS local
// (`const EventEmitter = require("events")`) is lowered inside the CJS
// wrapper, so it becomes a per-evaluation class. When it is constructed as
// the parent of a subclass, its inherited methods must still read its
// #private fields. Mirrors @redis/client's RedisClient/RedisSocket shape,
// including `attachConfig`'s `class extends BaseClass {}` built inside a
// helper whose `extends` operand is a parameter (a per-call class expression
// since #11042).
//
// Node runs test-files/*.ts as ESM (the repo's package.json has
// "type": "module"), where a bare `require` does not exist. So this file
// spells the CJS wrapper out: the module body is a function and the
// `require`d EventEmitter is a local of it -- the exact scope shape Perry's
// wrapper gives a CommonJS module. The literal bare-`require` form is covered
// by crates/perry/tests/private_brand_ancestor_evaluation.rs.
import events from "node:events";

function cjsModuleBody(require: (id: string) => any) {
  const EventEmitter = require("events");

  class Client {
    #socket: any;
    #queue: string[] = [];
    constructor() {
      this.#socket = new EventEmitter();
    }
    kind() {
      return typeof this.#socket;
    }
    send(cmd: string) {
      this.#queue.push(cmd);
      this.#socket.emit("data", cmd);
      return this.#queue.length;
    }
    onData(fn: (s: string) => void) {
      this.#socket.on("data", fn);
      return this;
    }
    static hasSocket(o: any) {
      return #socket in o;
    }
  }

  class Sub extends Client {}

  try {
    console.log("private", new Sub().kind());
  } catch (e) {
    console.log("private threw", (e as Error).message);
  }
  console.log("direct", new Client().kind());
  console.log("dynamic", new (Sub as any)().kind());

  const seen: string[] = [];
  const s = new Sub().onData((d) => seen.push(d));
  console.log("send", s.send("PING"), s.send("SET k v"), seen.join("|"));
  console.log("brand", Client.hasSocket(s), Client.hasSocket({}));

  function attachConfig({ BaseClass, commands }: { BaseClass: any; commands: Record<string, string> }) {
    const Class = class extends BaseClass {};
    for (const [name, reply] of Object.entries(commands)) {
      Class.prototype[name] = function (this: any) {
        return this.send(reply);
      };
    }
    return Class;
  }
  const Attached = attachConfig({ BaseClass: Client, commands: { PING: "PONG" } });
  const c: any = new Attached();
  console.log("attached", c.kind(), c.PING(), Client.hasSocket(c), c instanceof Client);

  class Socket extends EventEmitter {
    #connected = false;
    connect() {
      this.#connected = true;
      this.emit("connect");
      return this.#connected;
    }
  }
  class RedisLike {
    #socket: Socket;
    constructor() {
      this.#socket = new Socket();
    }
    connect() {
      let events = 0;
      this.#socket.on("connect", () => events++);
      const ok = this.#socket.connect();
      return ok + ":" + events;
    }
  }
  const Redis = attachConfig({ BaseClass: RedisLike, commands: {} });
  console.log("redis-like", new Redis().connect());
}

cjsModuleBody((id: string) => (id === "events" ? events : undefined));
