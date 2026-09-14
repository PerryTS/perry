// A positive inherited toJSON lookup on every stringify call.
const count = Number(process.argv[2] || "100000");
const object: any = Object.create({ toJSON() { return 7; } });
let sum = 0;
for (let i = 0; i < count; i++) {
  const text = JSON.stringify(object);
  if (text !== "7") throw new Error("toJSON mismatch");
  sum += text.length;
}
if (sum !== count) throw new Error("stringify count mismatch");
console.log(sum);
