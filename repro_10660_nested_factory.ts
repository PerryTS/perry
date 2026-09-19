// Adds the one remaining ingredient not yet tried: a SECOND, unrelated
// class with its OWN `class extends Base {}` factory-expression pattern,
// constructed FROM INSIDE the first class's constructor (mirroring
// `Object.defineProperty(this, "multi", { value: this.MULTI })`, where the
// `MULTI` getter constructs a `RedisClientMultiCommand` via ITS OWN
// `.extend()` factory while RedisClient's OWN constructor is still running).
"use strict";
import { EventEmitter } from "events";

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

// ---- second, unrelated class with its own extend()-factory pattern ----
var _Multi_b: any;
class MultiBase {
  static extend(): any {
    return class extends _Multi_b {
      constructor(...args: any[]) {
        super(...args);
      }
    };
  }
  tag = "multi";
}
_Multi_b = MultiBase;

// ---- primary class, mirrors RedisClient exactly ----
var _RedisClient_instances: WeakSet<any>,
  _a: any,
  _RedisClient_options: WeakMap<any, any>,
  _RedisClient_socket: WeakMap<any, any>,
  _RedisClient_queue: WeakMap<any, any>,
  _RedisClient_isolationPool: WeakMap<any, any>,
  _RedisClient_v4: WeakMap<any, any>,
  _RedisClient_selectedDB: WeakMap<any, any>,
  _RedisClient_initiateOptions: Function,
  _RedisClient_initiateQueue: Function,
  _RedisClient_initiateSocket: Function,
  _RedisClient_initiateIsolationPool: Function,
  _RedisClient_legacyMode: Function,
  _RedisClient_legacySendCommand: Function,
  _RedisClient_defineLegacyCommand: Function,
  _RedisClient_pingTimer: WeakMap<any, any>,
  _RedisClient_setPingTimer: Function,
  _RedisClient_sendCommand: Function,
  _RedisClient_pubSubCommand: Function,
  _RedisClient_tick: Function,
  _RedisClient_addMultiCommands: Function,
  _RedisClient_destroyIsolationPool: Function;

class RedisClient extends EventEmitter {
  get MULTI(): any {
    // Mirrors `this.MULTI` being a bound-getter-style property that
    // constructs a MultiCommand instance through ITS OWN extend() factory,
    // evaluated WHILE RedisClient's own constructor is still on the stack.
    const MultiCls = MultiBase.extend();
    return new MultiCls();
  }
  constructor(options: any) {
    super();
    _RedisClient_instances.add(this);
    _RedisClient_options.set(this, void 0);
    _RedisClient_socket.set(this, void 0);
    _RedisClient_queue.set(this, void 0);
    _RedisClient_isolationPool.set(this, void 0);
    _RedisClient_v4.set(this, {});
    _RedisClient_selectedDB.set(this, 0);
    _RedisClient_pingTimer.set(this, void 0);
    Object.defineProperty(this, "multi", {
      enumerable: true,
      configurable: true,
      writable: true,
      value: this.MULTI,
    });
    __classPrivateFieldSet(this, _RedisClient_options, options, "f");
    __classPrivateFieldSet(
      this,
      _RedisClient_queue,
      __classPrivateFieldGet(this, _RedisClient_instances, "m", _RedisClient_initiateQueue).call(this),
      "f",
    );
    __classPrivateFieldSet(
      this,
      _RedisClient_socket,
      __classPrivateFieldGet(this, _RedisClient_instances, "m", _RedisClient_initiateSocket).call(this),
      "f",
    );
    __classPrivateFieldSet(
      this,
      _RedisClient_isolationPool,
      __classPrivateFieldGet(this, _RedisClient_instances, "m", _RedisClient_initiateIsolationPool).call(this),
      "f",
    );
    __classPrivateFieldGet(this, _RedisClient_instances, "m", _RedisClient_legacyMode).call(this);
  }
  static extend(): any {
    return class extends _a {
      constructor(...args: any[]) {
        super(...args);
      }
    };
  }
  static create(options: any): RedisClient {
    return new (_a.extend())(options);
  }
  async connect(): Promise<RedisClient> {
    __classPrivateFieldSet(
      this,
      _RedisClient_isolationPool,
      __classPrivateFieldGet(this, _RedisClient_isolationPool, "f") ??
        __classPrivateFieldGet(this, _RedisClient_instances, "m", _RedisClient_initiateIsolationPool).call(this),
      "f",
    );
    await __classPrivateFieldGet(this, _RedisClient_socket, "f").connect();
    return this;
  }
}
_a = RedisClient;
_RedisClient_instances = new WeakSet();
_RedisClient_options = new WeakMap();
_RedisClient_socket = new WeakMap();
_RedisClient_queue = new WeakMap();
_RedisClient_isolationPool = new WeakMap();
_RedisClient_v4 = new WeakMap();
_RedisClient_selectedDB = new WeakMap();
_RedisClient_initiateOptions = function _RedisClient_initiateOptions(this: any, options: any) {
  return options;
};
_RedisClient_initiateQueue = function _RedisClient_initiateQueue(this: any) {
  return [];
};
_RedisClient_initiateSocket = function _RedisClient_initiateSocket(this: any) {
  return {
    fake: true,
    async connect() {
      return this;
    },
  };
};
_RedisClient_initiateIsolationPool = function _RedisClient_initiateIsolationPool(this: any) {
  return { pool: true };
};
_RedisClient_legacyMode = function _RedisClient_legacyMode(this: any) {
  return undefined;
};
_RedisClient_legacySendCommand = function _RedisClient_legacySendCommand(this: any) {
  return undefined;
};
_RedisClient_defineLegacyCommand = function _RedisClient_defineLegacyCommand(this: any) {
  return undefined;
};
_RedisClient_pingTimer = new WeakMap();
_RedisClient_setPingTimer = function _RedisClient_setPingTimer(this: any) {
  return undefined;
};
_RedisClient_sendCommand = function _RedisClient_sendCommand(this: any) {
  return undefined;
};
_RedisClient_pubSubCommand = function _RedisClient_pubSubCommand(this: any) {
  return undefined;
};
_RedisClient_tick = function _RedisClient_tick(this: any) {
  return undefined;
};
_RedisClient_addMultiCommands = function _RedisClient_addMultiCommands(this: any) {
  return undefined;
};
_RedisClient_destroyIsolationPool = function _RedisClient_destroyIsolationPool(this: any) {
  return undefined;
};

async function main() {
  const client = RedisClient.create({ url: "x" });
  client.on("error", (_e: any) => {});
  try {
    await client.connect();
    console.log("RESULT: PASS");
  } catch (e: any) {
    console.log("RESULT: ERROR " + (e && e.message ? e.message : String(e)));
  }
}

main();
