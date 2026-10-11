function run(v: any) { return typeof v === 'object' ? 1 : 0; }
const objs: any[] = [{a: 1}, {}, function () {}, new Date(0), Symbol.for('x')];
const count = Number(process.argv[2] || '200000');
let sum = 0;
for (let i = 0; i < count; i++) sum += run(objs[i % 5]);
console.log(sum);
