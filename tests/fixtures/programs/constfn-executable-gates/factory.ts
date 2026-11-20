function make(value: number) { return { m: () => value, x: value }; }
function invoke(receiver: any) { return receiver.m(); }
const left = make(17), right = make(29);
let sum = 0;
for (let i = 0; i < 4096; i++) sum += invoke(i % 2 ? right : left);
console.log(sum, left.m !== right.m, invoke(left), invoke(right));
left.m = () => 103;
console.log(invoke(left), invoke(right));
Object.defineProperty(left, 'm', { get: () => () => 202, configurable: true });
console.log(invoke(left), invoke(right));
delete (left as any).m;
console.log(typeof left.m, invoke(right));
