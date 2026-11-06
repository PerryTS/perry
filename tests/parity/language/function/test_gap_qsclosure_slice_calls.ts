// Dynamic argument slices retain padding, receivers, rest and wide calls.
const receiver = { tag: "receiver" };
function fixed(this: any, a: any, b: any, c: any) {
  return [this.tag, a, b, c === undefined ? "missing" : c].join(":");
}
const functions: any[] = [fixed];
for (const args of [[], [1], [1, 2], [1, 2, 3], [1, 2, 3, 4]]) {
  console.log(Reflect.apply(functions[0], receiver, args));
}
function rest(this: any, a: any, ...tail: any[]) {
  return this.tag + ":" + a + ":" + tail.join(",");
}
console.log(Reflect.apply(rest, receiver, [1, 2, 3, 4]));
function all(this: any) {
  return this.tag + ":" + arguments.length + ":" + arguments[17];
}
const wide = Array.from({ length: 20 }, (_, i) => i + 1);
console.log(Reflect.apply(all, receiver, wide));
console.log(Reflect.apply(functions[0], receiver, wide));
const proxy = new Proxy(fixed, {
  apply(target, self, args) { return "proxy:" + Reflect.apply(target, self, args); },
});
console.log(Reflect.apply(proxy, receiver, [8, 9]));
const bound = fixed.bind(receiver, 10);
console.log(Reflect.apply(bound, { tag: "ignored" }, [11]));
function forwarding(fn: any) {
  return function(this: any) { return fn.apply(this, arguments); };
}
console.log(Reflect.apply(forwarding(fixed), receiver, [12, 13]));
const patched: any = function() {};
patched.apply = function(self: any, args: any) {
  return self.tag + ":" + Object.prototype.toString.call(args) + ":" + args.length;
};
console.log(Reflect.apply(forwarding(patched), receiver, [14, 15, 16]));
