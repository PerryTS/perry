const proto: any = Object.prototype;
proto.length = 2;
proto[0] = 'proto';
const plain: any = {};
console.log('prototype', Array.prototype.join.call(plain, ','));
const nil: any = Object.create(null);
console.log('null proto', Array.prototype.join.call(nil, ','));
const own: any = { length: undefined };
console.log('own undefined', Array.prototype.join.call(own, ','));
const setterOnly: any = { set length(v: number) {} };
console.log('setter only', Array.prototype.join.call(setterOnly, ','));
delete proto.length;
delete proto[0];
let calls = 0;
const getter: any = { get length() { calls++; return '2'; }, 0: 'a', 1: 'b' };
console.log('own getter', Array.prototype.join.call(getter, ','), calls);
console.log('boolean length', Array.prototype.join.call({ length: true, 0: 'a' }, ','));
function fn(a: any) {}
try { Array.prototype.push.call(fn, 'x'); } catch (e: any) { console.log(e.name, fn.length, (fn as any)[1]); }
try { Array.prototype.pop.call(fn); } catch (e: any) { console.log(e.name, fn.length, (fn as any)[0]); }
