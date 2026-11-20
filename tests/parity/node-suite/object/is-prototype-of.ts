function logCall(label: string, fn: () => boolean) {
  try {
    console.log(label, "ok", fn());
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message, err instanceof TypeError);
  }
}

const proto = { marker: true };
const child = Object.create(proto);
const grandchild = Object.create(child);
const unrelated = { marker: true };

logCall("direct child", () => proto.isPrototypeOf(child));
logCall("direct grandchild", () => proto.isPrototypeOf(grandchild));
logCall("direct reversed", () => child.isPrototypeOf(proto));
logCall("direct unrelated", () => proto.isPrototypeOf(unrelated));
logCall("direct self", () => proto.isPrototypeOf(proto));
logCall("direct null arg", () => proto.isPrototypeOf(null));
logCall("direct primitive arg", () => proto.isPrototypeOf(42));

logCall("call child", () => Object.prototype.isPrototypeOf.call(proto, child));
logCall("call grandchild", () => Object.prototype.isPrototypeOf.call(proto, grandchild));
logCall("call unrelated", () => Object.prototype.isPrototypeOf.call(proto, unrelated));
logCall("primitive receiver", () => (42 as any).isPrototypeOf(child));
