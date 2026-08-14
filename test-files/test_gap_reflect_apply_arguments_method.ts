// #8036: a class method that forwards its `arguments` object through
// Reflect.apply must preserve every supplied value. Next 16's ProxyTracer uses
// this exact shape for startActiveSpan; the direct virtual-dispatch path used
// to pack synthetic `arguments` like an ordinary trailing rest parameter and
// therefore supplied an empty list.
class Target {
  start(_name: string, _options: object, callback: (span: number) => number) {
    return callback(42);
  }
}

class ProxyTarget {
  target = new Target();

  start(name: string, options: object, callback: (span: number) => number, context?: unknown) {
    return Reflect.apply(this.target.start, this.target, arguments);
  }
}

let called = false;
// Keep the receiver dynamic so this exercises the class-id dispatch tower used
// by Next's minified tracer path, not only the statically typed direct call.
function invokeStart(receiver: any, callback: (span: number) => number) {
  return receiver.start("route", {}, callback);
}

const value = invokeStart(new ProxyTarget(), (span) => {
  called = span === 42;
  return 8036;
});

console.log(called, value);
