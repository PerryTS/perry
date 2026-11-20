// #3726: node:crypto exposes `Cipheriv` / `Decipheriv` as constructor
// exports alongside the `createCipheriv()` / `createDecipheriv()` factory
// helpers. Mirror Node's observable module shape: both read as callable
// functions with length 4 and a matching `name`.
import * as crypto from "node:crypto";

console.log("Cipheriv typeof:", typeof crypto.Cipheriv);
console.log("Decipheriv typeof:", typeof crypto.Decipheriv);
console.log("createCipheriv typeof:", typeof crypto.createCipheriv);
console.log("createDecipheriv typeof:", typeof crypto.createDecipheriv);
console.log("Cipheriv name:", crypto.Cipheriv.name);
console.log("Decipheriv name:", crypto.Decipheriv.name);
console.log("Cipheriv length:", crypto.Cipheriv.length);
console.log("Decipheriv length:", crypto.Decipheriv.length);
