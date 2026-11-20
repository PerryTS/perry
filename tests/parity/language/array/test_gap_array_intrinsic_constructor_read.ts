export {};
// Read the inherited constructor before a free Array read builds the realm.
const cold: any = [];
const constructor: any = cold.constructor;
console.log("cold", typeof constructor, constructor === Array);
const lazy: any = JSON.parse("[1,2]");
console.log("parsed", lazy.constructor === constructor);
function read(value: any): any { return value.constructor; }
for (let i = 0; i < 20; i++) {
  if (read(cold) !== constructor) throw new Error("initial constructor");
}
const replacement: any = function Replacement() {};
(globalThis as any).Array = replacement;
console.log("binding replaced", read(cold) === constructor, read(lazy) === constructor);
(globalThis as any).Array = constructor;
