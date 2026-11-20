import consumersDefault, {
  arrayBuffer,
  blob,
  buffer,
  bytes,
  json,
  text,
} from "node:stream/consumers";
import * as consumers from "node:stream/consumers";

const expected = ["arrayBuffer", "blob", "buffer", "bytes", "json", "text"] as const;

console.log("namespace default type:", typeof consumers.default);
console.log("default import identity:", consumersDefault === consumers.default);
console.log("default is namespace:", consumersDefault === consumers);
console.log(
  "default has own default:",
  Object.prototype.hasOwnProperty.call(consumersDefault, "default"),
);

for (const name of expected) {
  console.log(
    `${name}:`,
    typeof consumers[name],
    typeof consumersDefault[name],
    consumers[name] === consumersDefault[name],
  );
}

console.log("named identities:", arrayBuffer === consumers.arrayBuffer, blob === consumers.blob);
console.log("more named identities:", buffer === consumers.buffer, bytes === consumers.bytes);
console.log("json text identities:", json === consumers.json, text === consumers.text);
