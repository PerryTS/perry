import { webcrypto } from "node:crypto";

void webcrypto.subtle;

(process as any).emitWarning = () => undefined;

console.log("direct:", SubtleCrypto.supports("digest", "SHA-256"));
console.log("computed:", SubtleCrypto["supports"]("digest", "SHA-256"));

const constructorAlias = SubtleCrypto;
console.log("alias:", constructorAlias.supports("digest", "SHA-256"));

const supports = SubtleCrypto.supports;
console.log("extracted call:", supports.call(SubtleCrypto, "digest", "SHA-256"));

const order: string[] = [];
function receiver() {
  order.push("receiver");
  return SubtleCrypto;
}
function operation() {
  order.push("operation");
  return "digest";
}
function algorithm() {
  order.push("algorithm");
  return "SHA-256";
}
console.log("evaluated:", receiver().supports(operation(), algorithm()));
console.log("order:", order.join(","));

function shadowed() {
  const SubtleCrypto = {
    marker: "local",
    supports(operation: string, algorithm: string) {
      return this.marker === "local" && operation === "digest" && algorithm === "SHA-256";
    },
  };
  console.log("shadowed:", SubtleCrypto.supports("digest", "SHA-256"));
}
shadowed();
