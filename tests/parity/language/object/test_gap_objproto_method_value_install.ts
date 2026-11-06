// An Object.prototype method read as a value is that method: a call of it on
// any receiver runs it there (`__defineGetter__` installs on `box`).
class Box {
  witness = 1;
}
const box: any = new Box();
const plain: any = {};
function cb() {
  return 2;
}
box.m = plain.__defineGetter__;
box.m("witness", cb);
console.log(box.witness);
class Pad {
  v = 1;
}
const pad: any = new Pad();
let got: any = null;
const set = plain.__defineSetter__;
set.call(pad, "v", function (x: any) {
  got = x;
});
pad.v = 9;
console.log(got, pad.v);
const ts = plain.toString;
console.log(ts.call(new Box()), typeof plain.valueOf, plain.valueOf.call(box) === box);
const look = plain.__lookupGetter__;
console.log(look.call(box, "witness") === cb);
