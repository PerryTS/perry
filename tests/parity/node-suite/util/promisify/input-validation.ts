import { callbackify, promisify } from "node:util";

const values: Array<[string, any]> = [
  ["undefined", undefined],
  ["null", null],
  ["number", 0],
  ["string", "x"],
  ["object", {}],
  ["array", []],
  ["promise", Promise.resolve(1)],
];

function probe(label: string, adapter: (value: any) => any, value: any) {
  try {
    const out = adapter(value);
    console.log(label, "ok", typeof out);
  } catch (err: any) {
    console.log(label, "throw", err.name, err.code, err instanceof TypeError);
  }
}

for (const [label, value] of values) {
  probe(`promisify ${label}`, promisify, value);
  probe(`callbackify ${label}`, callbackify, value);
}

function nodeback(cb: (err: any, value: number) => void) {
  cb(null, 1);
}

async function asyncValue() {
  return 1;
}

function custom(cb: (err: any, value: string) => void) {
  cb(null, "unused");
}
(custom as any)[promisify.custom] = () => Promise.resolve("custom");

console.log("promisify callable", typeof promisify(nodeback));
console.log("callbackify callable", typeof callbackify(asyncValue));
console.log("promisify custom", typeof promisify(custom));
