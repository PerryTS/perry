class X extends TransformStream {}
class Explicit extends TransformStream { constructor() { super(); } }
const x = new X();
const explicit = new Explicit();
console.log("subclass", x instanceof X, x instanceof TransformStream);
console.log("explicit", explicit instanceof Explicit, explicit instanceof TransformStream);
console.log("prototype-parent", Object.getPrototypeOf(X.prototype) === TransformStream.prototype);
class R extends ReadableStream {}
class W extends WritableStream {}
const r = new R();
const w = new W();
console.log("stream-subclasses", r instanceof R, r instanceof ReadableStream, w instanceof W, w instanceof WritableStream);
const Alias = TransformStream;
console.log("alias", x instanceof Alias);
console.log("prototype", Object.create(X.prototype) instanceof TransformStream);
console.log("direct", new TransformStream() instanceof TransformStream);
console.log("unrelated", {} instanceof TransformStream, new ReadableStream() instanceof TransformStream);
console.log("other-streams", new ReadableStream() instanceof ReadableStream, new WritableStream() instanceof WritableStream);
const parent = Object.getPrototypeOf(X.prototype);
Object.setPrototypeOf(X.prototype, Object.prototype);
console.log("relinked", x instanceof X, x instanceof TransformStream);
Object.setPrototypeOf(X.prototype, parent);
console.log("restored", x instanceof TransformStream);
