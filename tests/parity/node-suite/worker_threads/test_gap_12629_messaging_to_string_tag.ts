// Refs #12629: MessageChannel, MessagePort and BroadcastChannel instances
// report their interface through @@toStringTag, like every other web builtin.
const m = new MessageChannel();
const b = new BroadcastChannel("tag-test");
for (const [name, value, C] of [
  ["MessageChannel", m, MessageChannel],
  ["MessagePort", m.port1, MessagePort],
  ["BroadcastChannel", b, BroadcastChannel],
] as [string, any, any][]) {
  const d = Object.getOwnPropertyDescriptor(C.prototype, Symbol.toStringTag)!;
  console.log(name, Object.prototype.toString.call(value), value[Symbol.toStringTag],
    d.value, d.writable, d.enumerable, d.configurable);
}
m.port1.close();
m.port2.close();
b.close();
