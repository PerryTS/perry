function run(v: any, ctor: any) { return v instanceof ctor ? 1 : 0; }
const objs: any[] = [{a: 1}, {}, new Date(0)];
const ctor: any = Date;
const count = Number(process.argv[2] || '200000');
let sum = 0;
for (let i = 0; i < count; i++) sum += run(objs[i % 3], ctor);
console.log(sum);
