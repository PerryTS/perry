// #12309: a named expression keeps its lexical inner name and outer class identity.
class Base { base = "base"; inherited() { return this.base; } }
function make() { return new Node("stored"); }
export const Node = (class Inner extends Base {
  value;
  initial = "initial";
  static label = "label";
  static { this.label += "!"; }
  constructor(value) { super(); this.value = value; }
  read() { return this.value + ":" + this.initial; }
  clone() { return new Inner(this.value); }
});
const value: any = make();
console.log(Node.name, Node.label, value.read(), value.inherited());
console.log(value.clone().read(), typeof Inner);
