import { scryptSync } from "node:crypto";

console.log("named scrypt:", scryptSync("password", "salt", 16).toString("hex"));
