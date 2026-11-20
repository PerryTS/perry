// One computed site must agree with Get after each shape mutation.
function read(o: any, k: string): any { return o[k]; }
function show(o: any, k: string): void { for (let i = 0; i < 20; i++) read(o, k); console.log(read(o, k)); }
const proto: any = { answer: 17 };
const mid: any = Object.create(proto); mid.pad = 1;
const o: any = Object.create(mid); o.pad = 2;
show(o, 'answer');
proto.answer = 18; show(o, 'answer');
o.answer = 19; show(o, 'answer');
delete o.answer; show(o, 'answer');
delete proto.answer; show(o, 'answer');
mid.answer = 21; show(o, 'answer');
let calls = 0;
Object.defineProperty(mid, 'answer', { configurable: true, get() { calls++; return this.pad + 30; } });
console.log(read(o, 'answer'), read(o, 'answer'), calls);
Object.setPrototypeOf(o, { answer: 41 }); show(o, 'answer');
const trap = new Proxy({ answer: 51 }, { get(t, k, receiver) { return Reflect.get(t, k, receiver) + 1; } });
Object.setPrototypeOf(o, trap); show(o, 'answer');
Object.setPrototypeOf(o, null); show(o, 'answer');
const deep: any = Object.create(Object.create(Object.create({})));
show(deep, 'later'); Object.getPrototypeOf(Object.getPrototypeOf(deep)).later = 61; show(deep, 'later');
console.log(read({ longComputedAnswer: 71 }, ('longComputed' + 'Answer').slice(0)));
for (let i = 0; i < 64; i++) read(deep, 'rotate' + i);
console.log(read(deep, 'later'));
