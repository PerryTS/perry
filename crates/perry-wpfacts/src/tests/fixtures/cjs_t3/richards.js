import { createRequire as __perry_cjs_create_require, isBuiltin as __perry_cjs_is_builtin } from 'node:module';
import _req_0 from './benchmark';


const _cjs = (function() {
function __perry_cjs_factory() {
    // #3527: `module`/`exports` are reassignable `var`s (mirroring Node, where
    // they are wrapper-function parameters), so CJS bodies that do
    // `var module = X` / `module = X` / `exports = X` — e.g. iconv-lite's
    // `for (...) { var module = modules[i]; mergeModules(exports, module); }`
    // — rebind the local instead of colliding with a `const`. The stable
    // `__cjs_module` is what the module actually exports, read back at the end;
    // a body reassigning its local `module` can't clobber it (Node holds the
    // real module ref the same way), so named/default-export resolution stays
    // correct regardless of what the body does to its `module` local.
    // #6769: the Node `Module` record surface. Set before user code so a
    // recursive load of this module observes the same shape Node exposes.
    //
    // ONE object literal, so the record is allocated with its final shape.
    // As eleven sequential assignments it walked eleven shape transitions and
    // eleven cold property stores — about 10k instructions each at module-init
    // time — in every CommonJS module in the graph. Folding only some of the
    // fields does not help: the trailing assignments keep transitioning the
    // record and the win disappears (measured at -0.08%), so the whole surface
    // folds or none of it does.
    //
    // `cjs_scaffolding.rs`'s `record_binding` matches this field list
    // positionally. Adding or reordering a field drops the record back to being
    // reported as a denied user candidate in the `Ptr<Shape>` report;
    // `preamble_canary_tests` is what catches that.
    const __cjs_module = {
        exports: {},
        __perry_cjs_record: true,
        __perry_cjs_factory: __perry_cjs_factory,
        id: "/root/CW2/f/sa/richards.js",
        path: "/root/CW2/f/sa",
        filename: "/root/CW2/f/sa/richards.js",
        loaded: false,
        children: [],
        parent: globalThis.__perry_cjs_pending_parent,
        paths: ["/root/CW2/f/sa" + '/node_modules'],
        require: undefined,
    };
    globalThis.__perry_cjs_pending_parent = undefined;
    // Node populates `module.parent` before the body evaluates, so link it
    // here rather than at the tail's registry publication.
    __perry_link_path_module_parent(__cjs_module);
    // Publish the record before user code, just as at final publication.
    // #10178: esbuild replaces module.exports before requiring its peer.
    // Holding the initial empty object loses that replacement at re-entry;
    // the runtime's existing record unwrap reads the current exports instead.
    // This needs no getter enumeration, copying, or extra publication calls.
    __perry_register_path_module_partial("/root/CW2/f/sa/richards.js", __cjs_module);
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // One `createRequire` instance for the whole program, not one per module.
    // It is used only for `.cache`, `.extensions` and loading builtins, and all
    // three are process-global in Node — nothing here is bound to this
    // module's path. The call costs ~117k instructions, so paying it per module
    // cost OpenCode's ~2,200 CJS modules a quarter of a billion instructions
    // before any user code ran.
    const __perry_cjs_base_require = (globalThis.__perry_cjs_shared_require
        || (globalThis.__perry_cjs_shared_require = __perry_cjs_create_require("/root/CW2/f/sa/richards.js")));
    __perry_cjs_base_require.cache["/root/CW2/f/sa/richards.js"] = __cjs_module;
    function __perry_cjs_require_error(kind, code, message) {
        const err = kind === 'type' ? new TypeError(message) : new Error(message);
        err.code = code;
        return err;
    }
    // Keep the original spelling: prefix-only builtins are packages when
    // requested without `node:` (#10410).
    function __perry_cjs_require_is_builtin(specifier) {
        return __perry_cjs_is_builtin(specifier);
    }
    // `isBuiltin` comes from `node:module` instead of a switch emitted into
    // EVERY CommonJS module. The switch carried both spellings of all 58
    // builtin names, so each module interned ~120 string constants and
    // initialised its own copy of the table before running a line of user
    // code. It was also more permissive than Node: `sea`, `sqlite`, `test` and
    // `test/reporters` are builtins only in their `node:` form, and the switch
    // accepted the bare spelling too. The runtime predicate agrees with Node
    // 26 on all 58 names in both spellings.

    // A path-registry initializer runs once, while deleting require.cache
    // requests another execution of a loaded CommonJS factory. Allocate this
    // module-bound loader only on that cold path; ordinary loads keep the
    // registry and live-record fast paths above.
    let __perry_cjs_reload_require;
    function __perry_cjs_refresh_path(path, value) {
        const cached = __perry_cjs_base_require.cache[path];
        if (cached !== undefined) return cached.exports;
        if (!__perry_has_path_module(path)) return value;
        if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/root/CW2/f/sa/richards.js");
        return __perry_cjs_reload_require(path);
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');
        if (specifier === './benchmark') { const __perry_cached = require.cache["/root/CW2/f/sa/benchmark.js"]; if (__perry_cached !== undefined) { const child = require.cache["/root/CW2/f/sa/benchmark.js"]; if (child) { if (child.parent === undefined) child.parent = module; if (module.children.indexOf(child) === -1) module.children.push(child); } return __perry_cached.exports; } return __perry_cjs_refresh_path("/root/CW2/f/sa/benchmark.js", _req_0); }
        // #sdxgen: Node.js built-in modules that were NOT hoisted as static
        // imports (see the builtin_requires filter above). Resolve them via
        // createRequire at runtime, which calls js_create_native_module_namespace
        // under the hood — the same path Node.js uses for require("process").
        if (__perry_cjs_require_is_builtin(specifier)) {
            return __perry_cjs_base_require(specifier);
        }
        // Native addons resolve through the same authenticated manifest and
        // cache as createRequire and process.dlopen, including computed paths.
        if (specifier.slice(-5) === '.node') {
            if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/root/CW2/f/sa/richards.js");
            return __perry_cjs_reload_require(specifier);
        }
        // Runtime `require(path)` of a module Perry AOT-compiled but that is
        // only reachable via a computed path. Next's webpack runtime uses both
        // absolute page paths and relative chunk paths (`./chunks/` + id).
        // Resolve the latter against this CJS module's directory before probing
        // the path registry, mirroring Node's per-module `require` binding.
        // `js_require_path_module` canonicalizes the joined path, so `./` and
        // `../` segments need no source-level normalization here.
        {
            // A runtime-COMPUTED *relative* specifier never matches that
            // registry, which is keyed by absolute source path. Next's
            // production webpack runtime does exactly this — `.next/server/
            // webpack-runtime.js` calls `require("./chunks/" + g.u(a))` — so
            // every lazy chunk missed and the App Route died at startup with
            // `Cannot find module './chunks/2.js'` even though that chunk WAS
            // compiled into the image. Statically-known relative specifiers are
            // already handled by the cases above; only computed ones reach
            // here, so join them against this module's own directory.
            //
            // The `./` prefix is stripped textually rather than left to
            // `std::fs::canonicalize`: that only normalizes a path that exists
            // on disk, and registration falls back to the raw string when it
            // does not, so `<dir>/./chunks/2.js` would miss `<dir>/chunks/2.js`
            // in exactly the deployed case where the sources are absent.
            var __perry_path_spec = specifier;
            if (specifier.charCodeAt(0) === 46) {
                if (specifier.charCodeAt(1) === 47) {
                    __perry_path_spec = "/root/CW2/f/sa" + '/' + specifier.slice(2);
                } else if (specifier.charCodeAt(1) === 46 && specifier.charCodeAt(2) === 47) {
                    __perry_path_spec = "/root/CW2/f/sa" + '/' + specifier;
                } else if (specifier === '.' || specifier === '..') {
                    // The bare directory specifiers carry no trailing
                    // separator, so the two prefix tests above miss them —
                    // yet Node accepts `require('.')` / `require('..')` and
                    // resolves them through the directory's `index.js` /
                    // package `main`, which `js_require_path_module` also
                    // does via its directory-candidate fallback. Without the
                    // join the key stays a bare `.` and can never hit.
                    __perry_path_spec = "/root/CW2/f/sa" + '/' + specifier;
                }
            }
            const __perry_path_mod = __perry_require_path_module(__perry_path_spec);
            if (__perry_path_mod !== undefined || __perry_has_path_module(__perry_path_spec)) return __perry_cjs_refresh_path(__perry_path_spec, __perry_path_mod);
        }
        // Runtime `require(absolutePath)` of a `.json` file (Next.js loads
        // manifests this way: `require(this.middlewareManifestPath)`). Node's
        // require reads + JSON.parses `.json` files; the statically-resolved
        // cases above only cover specifiers known at compile time, so a path
        // computed at runtime falls here. `.json` is pure data (no eval), so we
        // read it from disk and parse it. Uncompiled `.js` runtime require
        // stays unsupported — that would require evaluating arbitrary code.
        if ((specifier.charCodeAt(0) === 47 || (specifier.length > 2 && specifier.charCodeAt(1) === 58)) && specifier.slice(-5) === '.json') {
            return __perry_require_json_disk(specifier);
        }
        // A computed bare platform package can name a manifest-listed addon
        // through package.json main, without a .node suffix in the request.
        if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/root/CW2/f/sa/richards.js");
        return __perry_cjs_reload_require(specifier);
    }
    // No `defineProperty(require, 'name', ...)`: a `function require(...)`
    // declaration already carries exactly
    // {value:'require', writable:false, enumerable:false, configurable:true},
    // verified identical in Node 26 and Perry. The redundant install also gave
    // the require object OBJ_FLAG_HAS_DESCRIPTORS, which pushed every later
    // `require.resolve = ...` / `require.cache = ...` assignment onto the
    // descriptor-bearing store path (#10287) in every CommonJS module.
    require.resolve = function resolve(specifier, options) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        if (specifier === "./benchmark") return "/root/CW2/f/sa/benchmark.js";
        if (__perry_cjs_require_is_builtin(specifier)) return specifier;
        var __perry_nm_resolved = __perry_require_resolve_node_modules("/root/CW2/f/sa", specifier);
        if (__perry_nm_resolved !== undefined) return __perry_nm_resolved;
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    };
    require.resolve.paths = function paths(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    };
    // `cache` and `extensions` come straight from the createRequire instance:
    // the placeholder object literals they used to be initialised with were
    // overwritten on the very next line, so every CJS module allocated an
    // object plus three closures and immediately dropped them. At OpenCode's
    // ~2,200 CJS modules that is pure startup garbage.
    require.cache = __perry_cjs_base_require.cache;
    require.extensions = __perry_cjs_base_require.extensions;
    require.main = __perry_get_cjs_main_module();
    module.require = function moduleRequire(specifier) { return require(specifier); };

    // @ts-check
// The benchmark in its current state is a derivation from the SOM version,
// which is derived from Mario Wolczko's Smalltalk version of DeltaBlue.
//
// The original license details are availble here:
// http://web.archive.org/web/20050825101121/http://www.sunlabs.com/people/mario/java_benchmarking/index.html

const { Benchmark } = require('./benchmark');

const NO_TASK = null;
const NO_WORK = null;
const IDLER = 0;
const WORKER = 1;
const HANDLER_A = 2;
const HANDLER_B = 3;
const DEVICE_A = 4;
const DEVICE_B = 5;
const NUM_TYPES = 6;

const DEVICE_PACKET_KIND = 0;
const WORK_PACKET_KIND = 1;

const DATA_SIZE = 4;

const TRACING = false;

class RBObject {
  append(packet, queueHead) {
    packet.link = NO_WORK;
    if (NO_WORK === queueHead) {
      return packet;
    }

    let mouse = queueHead;
    let link;

    while (NO_WORK !== (link = mouse.link)) {
      mouse = link;
    }
    mouse.link = packet;
    return queueHead;
  }
}

class TaskState extends RBObject {
  constructor() {
    super();
    this.packetPending_ = false;
    this.taskWaiting_ = false;
    this.taskHolding_ = false;
  }

  isPacketPending() { return this.packetPending_; }

  isTaskHolding() { return this.taskHolding_; }

  isTaskWaiting() { return this.taskWaiting_; }

  setTaskHolding(b) { this.taskHolding_ = b; }

  setTaskWaiting(b) { this.taskWaiting_ = b; }

  setPacketPending(b) { this.packetPending_ = b; }

  packetPending() {
    this.packetPending_ = true;
    this.taskWaiting_ = false;
    this.taskHolding_ = false;
  }

  running() {
    this.packetPending_ = false;
    this.taskWaiting_ = false;
    this.taskHolding_ = false;
  }

  waiting() {
    this.packetPending_ = false;
    this.taskHolding_ = false;
    this.taskWaiting_ = true;
  }

  waitingWithPacket() {
    this.taskHolding_ = false;
    this.taskWaiting_ = true;
    this.packetPending_ = true;
  }

  isTaskHoldingOrWaiting() {
    return this.taskHolding_ || (!this.packetPending_ && this.taskWaiting_);
  }

  isWaitingWithPacket() {
    return this.packetPending_ && this.taskWaiting_ && !this.taskHolding_;
  }

  static createRunning() {
    const t = new TaskState();
    t.running();
    return t;
  }

  static createWaiting() {
    const t = new TaskState();
    t.waiting();
    return t;
  }

  static createWaitingWithPacket() {
    const t = new TaskState();
    t.waitingWithPacket();
    return t;
  }
}

class DeviceTaskDataRecord extends RBObject {
  constructor() {
    super();
    this.pending = NO_WORK;
  }
}

class HandlerTaskDataRecord extends RBObject {
  constructor() {
    super();
    this.workIn = NO_WORK;
    this.deviceIn = NO_WORK;
  }

  deviceInAdd(packet) {
    this.deviceIn = this.append(packet, this.deviceIn);
  }

  workInAdd(packet) {
    this.workIn = this.append(packet, this.workIn);
  }
}

class IdleTaskDataRecord extends RBObject {
  constructor() {
    super();
    this.control = 1;
    this.count = 10000;
  }
}

class Packet extends RBObject {
  constructor(link, identity, kind) {
    super();
    this.link = link;
    this.identity = identity;
    this.kind = kind;
    this.datum = 0;
    this.data = new Array(DATA_SIZE).fill(0);
  }
}

class TaskControlBlock extends TaskState {
  constructor(link, identity, priority, initialWorkQueue, initialState, privateData, fn) {
    super();
    this.link = link;
    this.identity = identity;
    this.priority = priority;
    this.input = initialWorkQueue;
    this.setPacketPending(initialState.isPacketPending());
    this.setTaskWaiting(initialState.isTaskWaiting());
    this.setTaskHolding(initialState.isTaskHolding());
    this.handle = privateData;
    this.function = fn;
  }

  addInputAndCheckPriority(packet, oldTask) {
    if (NO_WORK === this.input) {
      this.input = packet;
      this.setPacketPending(true);
      if (this.priority > oldTask.priority) { return this; }
    } else {
      this.input = this.append(packet, this.input);
    }
    return oldTask;
  }

  runTask() {
    let message;
    if (this.isWaitingWithPacket()) {
      message = this.input;
      this.input = message.link;
      if (NO_WORK === this.input) {
        this.running();
      } else {
        this.packetPending();
      }
    } else {
      message = NO_WORK;
    }
    return this.function(message, this.handle);
  }
}

class WorkerTaskDataRecord extends RBObject {
  constructor() {
    super();
    this.destination = HANDLER_A;
    this.count = 0;
  }
}

class Scheduler extends RBObject {
  constructor() {
    super();

    // init tracing
    this.layout = 0;

    // init scheduler
    this.queuePacketCount = 0;
    this.holdCount = 0;
    this.taskTable = new Array(NUM_TYPES).fill(NO_TASK);
    this.taskList = NO_TASK;

    this.currentTask = null;
    this.currentTaskIdentity = 0;
  }

  createDevice(identity, priority, workPacket, state) {
    const data = new DeviceTaskDataRecord();

    this.createTask(identity, priority, workPacket, state, data, (workArg, wordArg) => {
      const dataRecord = wordArg;
      let functionWork = workArg;
      if (NO_WORK === functionWork) {
        if (NO_WORK === (functionWork = dataRecord.pending)) {
          return this.markWaiting();
        }
        dataRecord.pending = NO_WORK;
        return this.queuePacket(functionWork);
      }
      dataRecord.pending = functionWork;
      if (TRACING) {
        this.trace(functionWork.datum);
      }
      return this.holdSelf();
    });
  }

  createHandler(identity, priority, workPacket, state) {
    const data = new HandlerTaskDataRecord();
    this.createTask(identity, priority, workPacket, state, data, (work, word) => {
      const dataRecord = word;
      if (NO_WORK !== work) {
        if (WORK_PACKET_KIND === work.kind) {
          dataRecord.workInAdd(work);
        } else {
          dataRecord.deviceInAdd(work);
        }
      }

      let workPacket_;
      if (NO_WORK === (workPacket_ = dataRecord.workIn)) {
        return this.markWaiting();
      }
      const count = workPacket_.datum;
      if (count >= DATA_SIZE) {
        dataRecord.workIn = workPacket_.link;
        return this.queuePacket(workPacket_);
      }
      let devicePacket;
      if (NO_WORK === (devicePacket = dataRecord.deviceIn)) {
        return this.markWaiting();
      }
      dataRecord.deviceIn = devicePacket.link;
      devicePacket.datum = workPacket_.data[count];
      workPacket_.datum = count + 1;
      return this.queuePacket(devicePacket);
    });
  }

  createIdler(identity, priority, work, state) {
    const data = new IdleTaskDataRecord();
    this.createTask(identity, priority, work, state, data, (workArg, wordArg) => {
      const dataRecord = wordArg;
      dataRecord.count -= 1;
      if (0 === dataRecord.count) {
        return this.holdSelf();
      }
      if (0 === (dataRecord.control & 1)) {
        dataRecord.control /= 2;
        return this.release(DEVICE_A);
      }
      dataRecord.control = (dataRecord.control / 2) ^ 53256;
      return this.release(DEVICE_B);
    });
  }

  createPacket(link, identity, kind) {
    return new Packet(link, identity, kind);
  }

  createTask(identity, priority, work, state, data, fn) {
    const t = new TaskControlBlock(
      this.taskList,
      identity,
      priority,
      work,
      state,
      data,
      fn
    );
    this.taskList = t;
    this.taskTable[identity] = t;
  }

  createWorker(identity, priority, workPacket, state) {
    const dataRecord = new WorkerTaskDataRecord();

    this.createTask(
      identity,
      priority,
      workPacket,
      state,
      dataRecord,
      (work, word) => {
        const data = word;
        if (NO_WORK === work) {
          return this.markWaiting();
        }

        data.destination = (HANDLER_A === data.destination) ? HANDLER_B : HANDLER_A;
        work.identity = data.destination;
        work.datum = 0;
        for (let i = 0; i < DATA_SIZE; i += 1) {
          data.count += 1;
          if (data.count > 26) { data.count = 1; }
          work.data[i] = 65 + data.count - 1;
        }
        return this.queuePacket(work);
      }
    );
  }

  start() {
    let workQ;

    this.createIdler(IDLER, 0, NO_WORK, TaskState.createRunning());
    workQ = this.createPacket(NO_WORK, WORKER, WORK_PACKET_KIND);
    workQ = this.createPacket(workQ, WORKER, WORK_PACKET_KIND);

    this.createWorker(WORKER, 1000, workQ, TaskState.createWaitingWithPacket());
    workQ = this.createPacket(NO_WORK, DEVICE_A, DEVICE_PACKET_KIND);
    workQ = this.createPacket(workQ, DEVICE_A, DEVICE_PACKET_KIND);
    workQ = this.createPacket(workQ, DEVICE_A, DEVICE_PACKET_KIND);

    this.createHandler(HANDLER_A, 2000, workQ, TaskState.createWaitingWithPacket());
    workQ = this.createPacket(NO_WORK, DEVICE_B, DEVICE_PACKET_KIND);
    workQ = this.createPacket(workQ, DEVICE_B, DEVICE_PACKET_KIND);
    workQ = this.createPacket(workQ, DEVICE_B, DEVICE_PACKET_KIND);

    this.createHandler(HANDLER_B, 3000, workQ, TaskState.createWaitingWithPacket());
    this.createDevice(DEVICE_A, 4000, NO_WORK, TaskState.createWaiting());
    this.createDevice(DEVICE_B, 5000, NO_WORK, TaskState.createWaiting());

    this.schedule();

    return this.queuePacketCount === 23246 && this.holdCount === 9297;
  }

  findTask(identity) {
    const t = this.taskTable[identity];
    if (NO_TASK === t) { throw new Error('findTask failed'); }
    return t;
  }

  holdSelf() {
    this.holdCount += 1;
    this.currentTask.setTaskHolding(true);
    return this.currentTask.link;
  }

  queuePacket(packet) {
    const t = this.findTask(packet.identity);
    if (NO_TASK === t) { return NO_TASK; }

    this.queuePacketCount += 1;

    packet.link = NO_WORK;
    packet.identity = this.currentTaskIdentity;
    return t.addInputAndCheckPriority(packet, this.currentTask);
  }

  release(identity) {
    const t = this.findTask(identity);
    if (NO_TASK === t) { return NO_TASK; }
    t.setTaskHolding(false);
    if (t.priority > this.currentTask.priority) {
      return t;
    }
    return this.currentTask;
  }

  trace(id) {
    this.layout -= 1;
    if (0 >= this.layout) {
      process.stdout.write('\n');
      this.layout = 50;
    }
    process.stdout.write(id);
  }

  markWaiting() {
    this.currentTask.setTaskWaiting(true);
    return this.currentTask;
  }

  schedule() {
    this.currentTask = this.taskList;
    while (NO_TASK !== this.currentTask) {
      if (this.currentTask.isTaskHoldingOrWaiting()) {
        this.currentTask = this.currentTask.link;
      } else {
        this.currentTaskIdentity = this.currentTask.identity;
        if (TRACING) { this.trace(this.currentTaskIdentity); }
        this.currentTask = this.currentTask.runTask();
      }
    }
  }
}

class Richards extends Benchmark {
  benchmark() {
    return (new Scheduler()).start();
  }

  verifyResult(result) {
    return result;
  }
}

exports.newInstance = () => new Richards();


    __cjs_module.loaded = true; __perry_register_path_module("/root/CW2/f/sa/richards.js", __cjs_module);
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;


export const newInstance = _cjs.newInstance;


