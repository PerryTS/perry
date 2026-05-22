import crypto from "node:crypto";
// X509Certificate is exposed as a constructor.
console.log("X509Certificate:", typeof crypto.X509Certificate === "function");
