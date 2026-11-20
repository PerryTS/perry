import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key1 = Buffer.from("0b".repeat(20), "hex");
const data1 = Buffer.from("4869205468657265", "hex");
console.log("case1 sha224:", crypto.createHmac("sha224", key1).update(data1).digest("hex"));
console.log("case1 sha256:", crypto.createHmac("sha256", key1).update(data1).digest("hex"));
console.log("case1 sha384:", crypto.createHmac("sha384", key1).update(data1).digest("hex"));
console.log("case1 sha512:", crypto.createHmac("sha512", key1).update(data1).digest("hex"));

const key2 = Buffer.from("4a656665", "hex");
const data2 = Buffer.from("7768617420646f2079612077616e7420666f72206e6f7468696e673f", "hex");
console.log("case2 sha256:", crypto.createHmac("sha256", key2).update(data2).digest("hex"));
console.log("case2 sha512:", crypto.createHmac("sha512", key2).update(data2).digest("hex"));
