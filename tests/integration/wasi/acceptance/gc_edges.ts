import { collect } from "perry/gc";
import { Buffer } from "node:buffer";

const error: any = new Error("retained error", { cause: { label: "cause" } });
error.extra = { label: "metadata" };
const map = new Map<string, any>([["entry", { label: "map" }]]);
const set = new Set<any>([{ label: "set" }]);
const regexp = new RegExp("a(b+)", "g");
const date: any = new Date(0);
date.extra = { label: "date" };
const closure: any = () => error.message;
closure.extra = { label: "closure" };
const owner = Buffer.from([10, 20, 30, 40]);
const view = owner.subarray(1, 3);
const parsed = JSON.parse('[{"label":"json"},[1,2,3]]');
for (let i = 0; i < 12; i++) collect();
console.log(error.message, error.cause.label, error.extra.label);
console.log(map.get("entry").label, Array.from(set)[0].label);
console.log(regexp.exec("abbb")[1], date.extra.label, closure(), closure.extra.label);
console.log(view.join(","), parsed[0].label, parsed[1].join(","));
Promise.resolve({ label: "promise" }).then(value => {
  collect();
  console.log(value.label);
});
