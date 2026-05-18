// V8-fallback helper used by `test_v8_class_instance_return.ts`. Mirrors
// the Effect/jose shape: a named export `Thing` that is itself an object
// (not a class function) whose `.make(v)` returns a class-instance with
// `.value` and `.doubled()`. The bug pre-#818 had `Thing.make(42)` fall
// to the `double_literal(0.0)` codegen stub.
class Box {
  constructor(v) { this.value = v; this._tag = 'Boxed'; }
  doubled() { return this.value * 2; }
  pipe(fn) { return fn(this); }
}

export const Thing = {
  make(v) { return new Box(v); },
  greet(name) { return 'hello ' + name; }
};
