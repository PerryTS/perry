const n = Number(process.argv[2] || "2000000");
const options = { prop: "value" };
class Computed {
  backing = 7;
  get [options.prop]() { return this.backing; }
  set [options.prop](v: number) { this.backing = v; }
}
const receiver: any = new Computed();
function read(o: any, count: number) {
  let checksum = 0;
  for (let i = 0; i < count; i++) checksum += o.value;
  return checksum;
}
console.log(read(receiver, n));
