const empty: any = JSON.parse("{}");
console.log("cold", JSON.stringify(empty));
const kept: any[] = [];
let failures = 0;
for (let i = 0; i < 2048; i++) {
  const object = JSON.parse("{}");
  const output = JSON.stringify(object);
  if (output !== "{}") failures++;
  kept.push(object);
  kept.push(output);
}
for (let i = 0; i < kept.length; i += 2) {
  if (JSON.stringify(kept[i]) !== kept[i + 1]) failures++;
}
console.log("retained", kept.length, failures);

let calls = "";
(Object.prototype as any).toJSON = function(key: string) {
  calls += "default:" + key + ",";
  return "default-result";
};
console.log("installed", JSON.stringify(empty));
delete (Object.prototype as any).toJSON;
console.log("removed", JSON.stringify(empty));

Object.defineProperty(Object.prototype, "toJSON", {
  configurable: true,
  get() {
    calls += "get,";
    return function(key: string) {
      calls += "getter-call:" + key + ",";
      return "getter-result";
    };
  }
});
console.log("getter", JSON.stringify(empty));
delete (Object.prototype as any).toJSON;
console.log("getter removed", JSON.stringify(empty));

const custom = JSON.parse("{}");
Object.setPrototypeOf(custom, {
  toJSON(key: string) { calls += "custom:" + key + ","; return "custom-result"; }
});
console.log("custom", JSON.stringify(custom));
console.log("null prototype", JSON.stringify(Object.create(null)));
const hidden = JSON.parse("{}");
Object.defineProperty(hidden, "toJSON", {
  value(key: string) { calls += "own:" + key + ","; return "own-result"; },
  enumerable: false
});
console.log("hidden callback", JSON.stringify(hidden));
let replacerCalls = 0;
console.log("replacer", JSON.stringify(empty, function(key: string, value: any) {
  replacerCalls++;
  return key === "" ? { changed: true } : value;
}), replacerCalls);
console.log("spacer", JSON.stringify(empty, null, 2));
empty.answer = 42;
console.log("mutated", JSON.stringify(empty), kept[1]);
console.log("calls", calls);
