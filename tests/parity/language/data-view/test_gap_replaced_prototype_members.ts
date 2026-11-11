// A call on a DataView / ArrayBuffer / SharedArrayBuffer is a [[Get]] of the
// member through the receiver's prototype chain, then a call: a member the
// program replaced on the prototype is the one that runs (node), and putting
// the original back restores the built-in. An own member shadows both.
const dv: any = new DataView(new ArrayBuffer(8));
const ab: any = new ArrayBuffer(8, { maxByteLength: 16 } as any);
const sab: any = new SharedArrayBuffer(8, { maxByteLength: 16 } as any);
const DVP: any = DataView.prototype;
const ABP: any = ArrayBuffer.prototype;
const SABP: any = SharedArrayBuffer.prototype;

function swap(proto: any, name: string, call: () => unknown) {
  const orig = proto[name];
  proto[name] = function (this: any, ...args: unknown[]) {
    return "replaced " + name + " " + (this === dv || this === ab || this === sab) + " " + args.join(",");
  };
  console.log(name, call());
  proto[name] = orig;
  const back = call();
  console.log(name, "back", back instanceof ArrayBuffer || back instanceof SharedArrayBuffer ? "buffer" : back);
}

swap(DVP, "getInt8", () => dv.getInt8(1));
swap(DVP, "setUint16", () => dv.setUint16(0, 258, true));
swap(DVP, "getFloat64", () => dv.getFloat64(0, true));
swap(DVP, "getBigInt64", () => typeof dv.getBigInt64(0));
swap(ABP, "slice", () => ab.slice(0, 4));
swap(ABP, "resize", () => ab.resize(12));
swap(SABP, "slice", () => sab.slice(1));
swap(SABP, "grow", () => sab.grow(12));
console.log(ab.byteLength, sab.byteLength);

// A non-function member: calling it throws a TypeError.
const orig = DVP.getUint8;
DVP.getUint8 = 5;
try { dv.getUint8(0); console.log("no throw"); } catch (e: any) { console.log(e.constructor.name); }
DVP.getUint8 = orig;
console.log(dv.getUint8(0));

// The built-in members still work when called reflectively.
const copy = ABP.slice.call(ab, 2);
console.log(copy.byteLength, SABP.slice.call(sab, 4).byteLength, DVP.getInt8.call(dv, 0));
ABP.resize.call(ab, 4);
console.log(ab.byteLength);
try { ABP.slice.call(sab); } catch (e: any) { console.log(e.constructor.name); }

// A subclass override and an own member shadow the prototype member.
class MyView extends DataView {
  getInt8(i: number) { return "sub " + i; }
}
const mv: any = new MyView(new ArrayBuffer(4));
console.log(mv.getInt8(2), mv.getUint8(0));
dv.getInt8 = () => "own";
console.log(dv.getInt8(0));
delete dv.getInt8;
console.log(dv.getInt8(0));
