import os from "node:os";

const nets = os.networkInterfaces();
const names = Object.keys(nets).sort();
console.log("names type:", Array.isArray(names), names.length >= 0);
for (const name of names.slice(0, 2)) {
  const list = nets[name] || [];
  console.log("iface:", typeof name, Array.isArray(list), list.length >= 0);
  const first: any = list[0];
  if (first) console.log("addr shape:", typeof first.address, typeof first.family, typeof first.internal);
}
