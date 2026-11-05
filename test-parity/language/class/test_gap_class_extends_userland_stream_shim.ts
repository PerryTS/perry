// winston native-compile wall (branch fix/winston-stream-subclass-state):
// `class Logger extends Transform` where `Transform` is NOT node:stream's
// builtin but a userland stream-shim's ES5 function constructor
// (`const { Transform } = require('readable-stream')`). readable-stream's
// hierarchy is `Transform -> Duplex -> Readable` wired via ES5
// `inherits()` (Object.create on the prototype), and the Readable ctor sets
// `this._readableState = new ReadableState(...)`. winston's logger.js then
// reads `const { pipes } = this._readableState`.
//
// Before the fix, perry routed `super()` for ANY parent textually named
// `Transform`/`Readable`/`Writable`/`Duplex` to the native node:stream shim
// (which never sets `_readableState`), so `this._readableState` was
// `undefined` and the destructure threw
// "Cannot convert undefined or null to object". The fix gates the native
// routing on `is_genuine_node_stream_parent` (the name must resolve to the
// `stream` native module) so a userland binding falls through to the
// dynamic `extends_expr` parent path, which runs the real constructor chain.

function inherits(ctor: any, superCtor: any) {
  ctor.super_ = superCtor;
  ctor.prototype = Object.create(superCtor.prototype, {
    constructor: {
      value: ctor,
      enumerable: false,
      writable: true,
      configurable: true,
    },
  });
}

// Base of the chain: sets the state objects the userland code reads.
function Readable(this: any, options: any) {
  this._readableState = { pipes: [], objectMode: !!(options && options.objectMode) };
  this.readable = true;
}

function Writable(this: any, options: any) {
  this._writableState = { finished: false };
  this.writable = true;
}

function Duplex(this: any, options: any) {
  Readable.call(this, options);
  Writable.call(this, options);
  this.allowHalfOpen = true;
}
inherits(Duplex, Readable);

function Transform(this: any, options: any) {
  Duplex.call(this, options);
  this._transformState = { transforming: false };
  // Mirror readable-stream: read the state set by the Readable ctor.
  this._readableState.needReadable = true;
}
inherits(Transform, Duplex);

// User ES6 class extending the userland ES5 `Transform`.
class Logger extends Transform {
  configured: boolean;
  constructor(options: any) {
    super({ objectMode: true });
    this.configured = true;
  }
  // Mirror winston logger.js line 695: read this._readableState directly.
  pipeCount(): number {
    const { pipes } = (this as any)._readableState;
    return pipes.length;
  }
}

const l: any = new Logger({});
console.log("readableState defined:", l._readableState !== undefined);
console.log("readableState.objectMode:", l._readableState.objectMode);
console.log("readableState.needReadable:", l._readableState.needReadable);
console.log("writableState defined:", l._writableState !== undefined);
console.log("transformState defined:", l._transformState !== undefined);
console.log("allowHalfOpen:", l.allowHalfOpen);
console.log("configured:", l.configured);
console.log("pipeCount:", l.pipeCount());
