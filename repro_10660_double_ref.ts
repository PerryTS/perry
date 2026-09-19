// Precisely mirror RedisClient.connect()'s FIRST statement: the SAME
// module-scope WeakMap identifier (`_Field`) is referenced TWICE in one
// expression statement -- once inside a nested __classPrivateFieldGet (the
// `??` LHS) and once as __classPrivateFieldSet's own validating receiver --
// inside an `async` method, after the field was validly set during a SYNC
// constructor via the same two-step pattern (.set(this, void 0) then
// __classPrivateFieldSet).
"use strict";
var __classPrivateFieldGet = function (receiver: any, state: any, kind: any, f: any) {
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a getter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver))
    throw new TypeError("Cannot read private member from an object whose class did not declare it");
  return kind === "m" ? f : kind === "a" ? f.call(receiver) : f ? f.value : state.get(receiver);
};
var __classPrivateFieldSet = function (receiver: any, state: any, value: any, kind: any, f: any) {
  if (kind === "m") throw new TypeError("Private method is not writable");
  if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a setter");
  if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver))
    throw new TypeError("Cannot write private member to an object whose class did not declare it");
  return (kind === "a" ? f.call(receiver, value) : f ? (f.value = value) : state.set(receiver, value)), value;
};

var _Client_instances: WeakSet<any>;
var _Client_options: WeakMap<any, any>;
var _Client_socket: WeakMap<any, any>;
var _Client_queue: WeakMap<any, any>;
var _Client_isolationPool: WeakMap<any, any>;
var _Client_v4: WeakMap<any, any>;
var _Client_selectedDB: WeakMap<any, any>;
var _Client_pingTimer: WeakMap<any, any>;
var _Client_initiateIsolationPool: Function;

class MiniEmitter {
  listeners: Record<string, Function[]> = {};
  on(name: string, cb: Function) {
    (this.listeners[name] ??= []).push(cb);
    return this;
  }
}

class Client extends MiniEmitter {
  constructor(options: any) {
    super();
    _Client_instances.add(this);
    _Client_options.set(this, void 0);
    _Client_socket.set(this, void 0);
    _Client_queue.set(this, void 0);
    _Client_isolationPool.set(this, void 0);
    _Client_v4.set(this, {});
    _Client_selectedDB.set(this, 0);
    _Client_pingTimer.set(this, void 0);
    __classPrivateFieldSet(this, _Client_options, options, "f");
    __classPrivateFieldSet(this, _Client_queue, [], "f");
    __classPrivateFieldSet(this, _Client_socket, { fake: true }, "f");
    __classPrivateFieldSet(
      this,
      _Client_isolationPool,
      __classPrivateFieldGet(this, _Client_instances, "m", _Client_initiateIsolationPool).call(this),
      "f",
    );
  }
  async connect(): Promise<Client> {
    __classPrivateFieldSet(
      this,
      _Client_isolationPool,
      __classPrivateFieldGet(this, _Client_isolationPool, "f") ??
        __classPrivateFieldGet(this, _Client_instances, "m", _Client_initiateIsolationPool).call(this),
      "f",
    );
    await Promise.resolve();
    return this;
  }
}
_Client_instances = new WeakSet();
_Client_options = new WeakMap();
_Client_socket = new WeakMap();
_Client_queue = new WeakMap();
_Client_isolationPool = new WeakMap();
_Client_v4 = new WeakMap();
_Client_selectedDB = new WeakMap();
_Client_pingTimer = new WeakMap();
_Client_initiateIsolationPool = function _Client_initiateIsolationPool(this: any) {
  return { pool: true };
};

async function main() {
  const client = new Client({ url: "x" });
  client.on("error", (_e: any) => {});
  try {
    await client.connect();
    console.log("RESULT: PASS");
  } catch (e: any) {
    console.log("RESULT: ERROR " + (e && e.message ? e.message : String(e)));
  }
}

main();
