class Anchor { get value() { return 1; } }
console.log(new Anchor().value);
const proto: any = Object.create(null);
for (let i = 0; i < 48; i++) proto['padding' + i] = i;
let calls = 0;
Object.defineProperty(proto, 'value', { configurable: true, get: function() { calls++; return this.payload; } });
const obj: any = Object.create(proto);
obj.payload = 7;
function read(o: any) { return o.value; }
let sum = 0;
for (let i = 0; i < 2000; i++) sum += read(obj);
console.log(sum, calls);
Object.defineProperty(proto, 'value', { configurable: true, get: function() { calls++; return this.payload + 10; } });
console.log(read(obj), calls);
Object.defineProperty(obj, 'value', { configurable: true, value: 23 });
console.log(read(obj), calls);
delete obj.value;
console.log(read(obj), calls);
delete proto.value;
console.log(read(obj), calls);
Object.defineProperty(proto, 'value', { configurable: true, get: function() { calls++; throw new Error('getter once'); } });
try { read(obj); } catch (e) { console.log((e as Error).message, calls); }
Object.setPrototypeOf(obj, { value: 31 });
console.log(read(obj), calls);
