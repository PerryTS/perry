function show(label: string, value: unknown) {
  console.log(label + ":", value);
}

class Empty {}

class Base {
  value() {
    return "base";
  }
}

class Child extends Base {
  child() {
    return "child";
  }
}

const empty = new Empty();
const child = new Child();
const base = new Base();

const emptyProto = Empty.prototype;
const baseProto = Base.prototype;
const childProto = Child.prototype;
const childRuntimeProto = Object.getPrototypeOf(child);

show("empty proto typeof", typeof emptyProto);
show("empty proto object", emptyProto !== null && typeof emptyProto === "object");
show("empty constructor identity", emptyProto.constructor === Empty);
show("empty proto owns constructor", Object.prototype.hasOwnProperty.call(emptyProto, "constructor"));
show("empty proto isPrototypeOf empty", emptyProto.isPrototypeOf(empty));

show("base proto typeof", typeof baseProto);
show("base constructor identity", baseProto.constructor === Base);
show("base own names", Object.getOwnPropertyNames(baseProto).join("|"));
show("base value type", typeof baseProto.value);
show("base proto parent object", Object.getPrototypeOf(baseProto) === Object.prototype);

show("child proto typeof", typeof childProto);
show("child constructor identity", childProto.constructor === Child);
show("child own names", Object.getOwnPropertyNames(childProto).join("|"));
show("child method type", typeof childProto.child);
show("child runtime proto", childRuntimeProto === childProto);
show("child proto parent", Object.getPrototypeOf(childProto) === baseProto);

show("base proto isPrototypeOf child", baseProto.isPrototypeOf(child));
show("child proto isPrototypeOf base", childProto.isPrototypeOf(base));
show("object proto isPrototypeOf child", Object.prototype.isPrototypeOf(child));
show("prototype identity stable", Child.prototype === childProto && Base.prototype === baseProto);
