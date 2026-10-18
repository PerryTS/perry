function exercise(value: any, key: string) {
  value.fixed = value.fixed + 1;
  value[key] = value[key] + 2;
  console.log("fixed" in value, value.fixed, value[key], value.run(4));
}
const value: any = JSON.parse('{"fixed":1,"chosen":3}');
value.run = function (n: number) { return this.fixed + n; };
exercise(value, "chosen");

class Parent {}
const parents: any = { base: Parent };
const parentKey = process.argv[2] || "base";
class Born extends parents[parentKey] { marker = 7; }
const born = new Born();
console.log(born.marker);

const entries: any = { left: (n: number) => n + 1, right: (n: number) => n + 2 };
const pick = process.argv[3] || 'left';
const invoke: any = entries[pick];
console.log(invoke(8));
