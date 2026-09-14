// A literal's variable can retain a forwarding alias after push grows it.
// Descriptor writes and reflection must agree on that receiver's owner key.
const values: any = [1, 2];
values.tag = 5;
values.push(3);
Object.defineProperty(values, "fixed", {
  value: 11, writable: false, enumerable: true, configurable: false,
});
const descriptor: any = Object.getOwnPropertyDescriptor(values, "fixed");
if (!descriptor || descriptor.value !== 11 || descriptor.writable !== false ||
    descriptor.enumerable !== true || descriptor.configurable !== false) {
  throw new Error("named descriptor attributes lost through a growth alias");
}
console.log("forwarded-named-descriptor", values.fixed, descriptor.writable,
  descriptor.enumerable, descriptor.configurable, values.length, values.tag);
