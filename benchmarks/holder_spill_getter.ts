const N = Number(process.argv[2] ?? 200000);
// Keep the existing program getter-name gate open for the measured name.
class Anchor { get value() { return 1; } }
const proto: any = Object.create(null);
for (let i = 0; i < 48; i++) proto['padding' + i] = i;
Object.defineProperty(proto, 'value', { configurable: true, get: function() { return this.payload; } });
const obj: any = Object.create(proto);
obj.payload = 7;
function read(o: any) { return o.value; }
let sum = new Anchor().value;
for (let i = 0; i < N; i++) sum += read(obj);
console.log(sum);
