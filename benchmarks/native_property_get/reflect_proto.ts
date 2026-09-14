// Same read through two explicitly recorded ordinary prototype links.
const count = Number(process.argv[2] || "200000");
const object: any = Object.create(Object.create({ value: 7 }));
let sum = 0;
for (let i = 0; i < count; i++) sum += Reflect.get(object, "value");
if (sum !== count * 7) throw new Error("prototype data read mismatch");
console.log(sum);
