// Inherited static calls must bind the class before a nested arrow captures this.
const proto: any = {
  ...{},
  key: "prototype",
  use() { return () => this.key; },
};
function Tag() {}
Object.setPrototypeOf(Tag, proto);
(Tag as any).key = "function";
class Service extends Tag {}
(Service as any).key = "class";
console.log((Tag as any).use()());
console.log((Service as any).use()());
