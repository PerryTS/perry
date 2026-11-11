// A growable SharedArrayBuffer (and a resizable ArrayBuffer) made through
// Reflect.construct, a constructor held in a variable, or a subclass is the
// same as one made with `new`: the { maxByteLength } options bag is read.
const viaReflect: any = Reflect.construct(SharedArrayBuffer, [4, { maxByteLength: 16 }]);
console.log(viaReflect.byteLength, viaReflect.growable, viaReflect.maxByteLength);
viaReflect.grow(8);
console.log(viaReflect.byteLength, new Uint8Array(viaReflect).length);
const C: any = SharedArrayBuffer;
const viaValue = new C(2, { maxByteLength: 4 });
console.log(viaValue.byteLength, viaValue.growable, viaValue.maxByteLength);
const fixed: any = Reflect.construct(SharedArrayBuffer, [4]);
console.log(fixed.byteLength, fixed.growable, fixed.maxByteLength);
const ab: any = Reflect.construct(ArrayBuffer, [4, { maxByteLength: 16 }]);
console.log(ab.byteLength, ab.resizable, ab.maxByteLength);
ab.resize(10);
console.log(ab.byteLength);
class Sub extends SharedArrayBuffer {}
const s: any = new Sub(2, { maxByteLength: 8 } as any);
console.log(s.byteLength, s.growable, s.maxByteLength, s instanceof Sub, s instanceof SharedArrayBuffer);
try { Reflect.construct(SharedArrayBuffer, [8, { maxByteLength: 4 }]); } catch (e: any) { console.log(e.constructor.name, e.message); }
try { viaReflect.grow(4); } catch (e: any) { console.log(e.constructor.name, e.message); }
try { fixed.grow(8); } catch (e: any) { console.log(e.constructor.name, e.message); }
