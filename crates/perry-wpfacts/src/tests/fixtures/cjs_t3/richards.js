import { cache as __perry_cjs_cache, main as __perry_cjs_main, extensions as __perry_cjs_extensions, setPending as __perry_cjs_set_pending, takePending as __perry_cjs_take_pending, link as __perry_cjs_link, requireComputed as __perry_cjs_require_computed, requireError as __perry_cjs_require_error } from 'perry:cjs';
import { createRequire as __perry_cjs_create_require } from 'node:module';
import _req_0 from './benchmark';
import { __perry_cjs_record as __perry_cjs_rec_0 } from './benchmark';
export var __perry_cjs_record;
let __perry_cjs_lk_0 = false;
function __perry_require_static_0() {
    
        const record = __perry_cjs_init(__perry_cjs_rec_0);
        if (__perry_cjs_lk_0 === false) {
            __perry_cjs_lk_0 = true;
            if (record.parent === undefined) record.parent = __perry_cjs_record;
            __perry_cjs_record.children.push(record);
        }
        return record.exports;
    
}



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
    const __cjs_module = {
        exports: __perry_declared_keys_object("newInstance"),
        __perry_cjs_record: true,
        id: "/t3/richards.js",
        path: "/t3",
        filename: "/t3/richards.js",
        loaded: false,
        children: [],
        parent: undefined,
        paths: ["/t3" + '/node_modules'],
        require: undefined
    };
    // The module's record binding: what a static edge of a requiring module
    // reads, from before the body, so a cycle sees the partial record.
    __perry_cjs_record = __cjs_module;
    
    
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // The program's `require.cache` (the support module's object, whose keys
    // are the graph's module paths): this module's key is a slot.
    const __perry_cjs_c = __perry_cjs_cache;
    __perry_cjs_c["/t3/richards.js"] = __cjs_module;

    // This module's own loader for the cold paths of `require` (a computed
    // specifier outside the graph, relative to this module), created on
    // first use: `createRequire` costs ~117k instructions. Only a program
    // that is not closed can compute a specifier or reload.
    let __perry_cjs_reload_require;
    function __perry_cjs_reloader() {
        
            throw __perry_cjs_require_error('error', 'ERR_PERRY_CJS_CLOSED', 'a closed program has no CommonJS loader');
        
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');
        if (specifier === "./benchmark") return __perry_require_static_0();
        // A specifier no case above resolved: builtins, native addons, a
        // compiled module reached by a computed path, a `.json` file, a
        // package outside the graph (`perry:cjs`'s `requireComputed`).
        // A closed program has no computed require and no reload (design
        // CJS_LINK §2.5): no loader surface is linked for it.
        
            throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
        
    }
    // No `defineProperty(require, 'name', ...)`: a `function require(...)`
    // declaration already carries exactly
    // {value:'require', writable:false, enumerable:false, configurable:true},
    // verified identical in Node 26 and Perry. The redundant install also gave
    // the require object OBJ_FLAG_HAS_DESCRIPTORS, which pushed every later
    // `require.resolve = ...` / `require.cache = ...` assignment onto the
    // descriptor-bearing store path (#10287) in every CommonJS module.
    const __perry_cjs_resolve = function resolve(specifier, options) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        if (specifier === "./benchmark") return "/t3/benchmark.js";
        // `require.resolve` is reachable only through `require` as a value,
        // which a closed program does not have.
        
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    };
    __perry_cjs_resolve.paths = function paths(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    };
    require.resolve = __perry_cjs_resolve;
    // `cache`, `extensions` and `main` are the program's (`perry:cjs`): Node's
    // loader reads `Module._cache`, never this property, so reassigning
    // `require.cache` does not redirect loading.
    require.cache = __perry_cjs_c;
    require.extensions = __perry_cjs_extensions;
    require.main = __perry_cjs_main;
    module.require = function moduleRequire(specifier) { return require(specifier); };

    // @ts-check
// The benchmark in its current state is a derivation from the SOM version,
// which is derived from Mario Wolczko's Smalltalk version of DeltaBlue.
//
// The original license details are availble here:
// http://web.archive.org/web/20050825101121/http://www.sunlabs.com/people/mario/java_benchmarking/index.html

const { Benchmark } = __perry_require_static_0();

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


    __cjs_module.loaded = true; 
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;


export const newInstance = _cjs.newInstance;

