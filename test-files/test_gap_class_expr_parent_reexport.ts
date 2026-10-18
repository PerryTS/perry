import {Parent} from "./fixtures/awe-parent-reexport/bridge.ts";
function make() {
  return class Child extends Parent {
    static marker = 1;
    read() { return super.read() + ":child"; }
  };
}
const maker = make;
const C = maker();
console.log(new C(7).read());
Object.defineProperty(Parent.prototype, "read", {
  configurable: true,
  value: function() { return "changed:" + this.value; }
});
console.log(new C(8).read());
