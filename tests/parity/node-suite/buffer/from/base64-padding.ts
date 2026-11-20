import { Buffer } from "node:buffer";

// base64 padding cases: 1/2/3 implicit-pad bytes + canonical full-block.
// Each input is the encoding of the corresponding plain string, with the
// '=' padding omitted to exercise the lenient decode path.
console.log("1B (TQ):", Buffer.from("TQ", "base64").toString("hex"));     // "M"
console.log("2B (TWE):", Buffer.from("TWE", "base64").toString("hex"));   // "Ma"
console.log("3B (TWFu):", Buffer.from("TWFu", "base64").toString("hex")); // "Man"
console.log("4B (TWFueQ):", Buffer.from("TWFueQ", "base64").toString("hex")); // "Many"
console.log("0B ():", Buffer.from("", "base64").length);
// With explicit padding, output must be byte-identical.
console.log("TQ== padded:", Buffer.from("TQ==", "base64").toString("hex"));
console.log("TWE= padded:", Buffer.from("TWE=", "base64").toString("hex"));
