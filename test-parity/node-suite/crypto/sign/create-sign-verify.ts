import crypto from "node:crypto";
// createSign / createVerify are factory functions for signature streams.
console.log("createSign:", typeof crypto.createSign === "function");
console.log("createVerify:", typeof crypto.createVerify === "function");
