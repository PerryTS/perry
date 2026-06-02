// #3141: node:v8 startupSnapshot helper namespace — shape + normal-runtime contract.
import * as v8 from "node:v8";

const s = v8.startupSnapshot;
console.log("startupSnapshot:", typeof s);
console.log("isBuildingSnapshot:", typeof s.isBuildingSnapshot);
console.log("addSerializeCallback:", typeof s.addSerializeCallback);
console.log("addDeserializeCallback:", typeof s.addDeserializeCallback);
console.log("setDeserializeMainFunction:", typeof s.setDeserializeMainFunction);
console.log("isBuildingSnapshot():", s.isBuildingSnapshot());

for (const [name, fn] of [
  ["addSerializeCallback", s.addSerializeCallback],
  ["addDeserializeCallback", s.addDeserializeCallback],
  ["setDeserializeMainFunction", s.setDeserializeMainFunction],
] as const) {
  try {
    fn(() => {});
    console.log(name + ": no throw");
  } catch (e: any) {
    console.log(name + ":", e.name, e.code);
  }
}
