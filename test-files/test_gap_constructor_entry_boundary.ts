// #12329: containment does not make first constructor stores shape-preserving.
function show(label, obj) {
  console.log(label, Object.keys(obj).join(","), JSON.stringify(obj));
}
class FirstStores {
  constructor(v) {
    this.a = v;
    this.b = v + 1;
    this.c = v + 2;
  }
}
show("first", new FirstStores(1));
let alias;
function publish(obj) { alias = obj; obj.extra = 7; }
function mutateAlias() { delete alias.a; alias.extra = 8; }
class Escapes {
  constructor(v) {
    this.a = v;
    publish(this);
    this.b = v + 1;
    mutateAlias();
    this.a = v + 2;
  }
}
show("escaped", new Escapes(1));
// This helper receives no `this`. It can still change first-store semantics
// by installing a setter on the known prototype.
function installSetter() {
  Object.defineProperty(PrototypeChange.prototype, "b", {
    set(value) { this.side = value; }, configurable: true
  });
}
class PrototypeChange {
  constructor(v) {
    this.a = v;
    installSetter();
    this.b = v + 1;
    this.c = v + 2;
  }
}
show("setter", new PrototypeChange(1));
class CallsThis {
  constructor(v) { this.a = v; this.init(); this.b = v + 1; }
  init() { this.extra = 7; }
}
show("receiver", new CallsThis(1));
class Conditional {
  constructor(flag) { if (flag) this.b = 2; this.a = 1; }
}
show("conditional-true", new Conditional(true));
show("conditional-false", new Conditional(false));
class Parent { constructor(v) { this.parent = v; } }
class Child extends Parent {
  constructor(v) { super(v); this.child = v + 1; this.parent = v + 2; }
}
show("subclass", new Child(1));
// A proxy prototype intercepts first stores even without a published receiver.
const proto = new Proxy({}, {
  set(target, key, value, receiver) {
    Object.defineProperty(receiver, "trap_" + key, {
      value, enumerable: true, configurable: true, writable: true
    });
    return true;
  }
});
class ProxyProto { constructor(v) { this.a = v; this.b = v + 1; } }
Object.setPrototypeOf(ProxyProto.prototype, proto);
show("proxy", new ProxyProto(1));
