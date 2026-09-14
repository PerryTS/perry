// Runtime Reflect.get entry, with an ordinary own data property.
const count = Number(process.argv[2] || "200000");
const object: any = { value: 7 };
let sum = 0;
for (let i = 0; i < count; i++) sum += Reflect.get(object, "value");
if (sum !== count * 7) throw new Error("own data read mismatch");
console.log(sum);
