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
