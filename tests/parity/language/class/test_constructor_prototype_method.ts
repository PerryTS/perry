function Box(this: any, value: number) {
  this.value = value;
}
(Box.prototype as any).getValue = function () {
  return this.value;
};
(Box.prototype as any).doubled = function () {
  return this.value * 2;
};

const b = new (Box as any)(5);
console.log(b.getValue()); // expect 5
console.log(b.doubled()); // expect 10
console.log(typeof (Box.prototype as any).getValue); // expect 'function'

// With computed property name
(Box.prototype as any)["triple"] = function () {
  return this.value * 3;
};
console.log(b.triple()); // expect 15
