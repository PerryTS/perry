// #12309: class syntax × field initialization × heritage × methods.
class Parent { inherited = "base"; baseMethod() { return this.inherited; } }
function makeC000(depth) {
  if (depth > 0) return makeC000(depth - 1);
  return new C000("stored");
}
const C000 = class {
  value;
  untouched;
  constructor(value) { this.value = value; }
};
const xC000: any = makeC000(1);
console.log("C000", xC000.value, xC000.untouched, typeof xC000.read);
function makeC001(depth) {
  if (depth > 0) return makeC001(depth - 1);
  return new C001("stored");
}
const C001 = class {
  value;
  untouched;
  constructor(value) { this.value = value; }
  read() { return this.value; }
};
const xC001: any = makeC001(1);
console.log("C001", xC001.value, xC001.untouched, typeof xC001.read, xC001.read());
function makeC010(depth) {
  if (depth > 0) return makeC010(depth - 1);
  return new C010("stored");
}
const C010 = class extends Parent {
  value;
  untouched;
  constructor(value) { super(); this.value = value; }
};
const xC010: any = makeC010(1);
console.log("C010", xC010.value, xC010.untouched, typeof xC010.read, xC010.baseMethod());
function makeC011(depth) {
  if (depth > 0) return makeC011(depth - 1);
  return new C011("stored");
}
const C011 = class extends Parent {
  value;
  untouched;
  constructor(value) { super(); this.value = value; }
  read() { return this.value; }
};
const xC011: any = makeC011(1);
console.log("C011", xC011.value, xC011.untouched, typeof xC011.read, xC011.read(), xC011.baseMethod());
function makeC100(depth) {
  if (depth > 0) return makeC100(depth - 1);
  return new C100("stored");
}
const C100 = class {
  value = "initial";
  untouched = "default";
  constructor(value) { this.value = value; }
};
const xC100: any = makeC100(1);
console.log("C100", xC100.value, xC100.untouched, typeof xC100.read);
function makeC101(depth) {
  if (depth > 0) return makeC101(depth - 1);
  return new C101("stored");
}
const C101 = class {
  value = "initial";
  untouched = "default";
  constructor(value) { this.value = value; }
  read() { return this.value; }
};
const xC101: any = makeC101(1);
console.log("C101", xC101.value, xC101.untouched, typeof xC101.read, xC101.read());
function makeC110(depth) {
  if (depth > 0) return makeC110(depth - 1);
  return new C110("stored");
}
const C110 = class extends Parent {
  value = "initial";
  untouched = "default";
  constructor(value) { super(); this.value = value; }
};
const xC110: any = makeC110(1);
console.log("C110", xC110.value, xC110.untouched, typeof xC110.read, xC110.baseMethod());
function makeC111(depth) {
  if (depth > 0) return makeC111(depth - 1);
  return new C111("stored");
}
const C111 = class extends Parent {
  value = "initial";
  untouched = "default";
  constructor(value) { super(); this.value = value; }
  read() { return this.value; }
};
const xC111: any = makeC111(1);
console.log("C111", xC111.value, xC111.untouched, typeof xC111.read, xC111.read(), xC111.baseMethod());
