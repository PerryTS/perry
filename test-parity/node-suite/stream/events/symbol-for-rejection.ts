import { captureRejectionSymbol } from "node:events";
// Symbol.for("nodejs.rejection") is captureRejectionSymbol.
const builtIn = Symbol.for("nodejs.rejection");
console.log("symbol matches:", builtIn === captureRejectionSymbol);
console.log("is symbol:", typeof captureRejectionSymbol === "symbol");
