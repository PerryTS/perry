function run(v: any) { return Object.keys(v).length; }
const objs: any[] = [{a: 1, b: 2, c: 3}, {name: 'x', roles: [1], meta: {}}];
const count = Number(process.argv[2] || '200000');
let sum = 0;
for (let i = 0; i < count; i++) sum += run(objs[i & 1]);
console.log(sum);
