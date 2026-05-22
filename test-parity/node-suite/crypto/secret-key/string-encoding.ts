import * as crypto from "node:crypto";

const hexKey = crypto.createSecretKey("68656c6c6f", "hex");
console.log("hex export:", hexKey.export().toString());
console.log("hex size:", hexKey.symmetricKeySize);

const base64Key = crypto.createSecretKey("aGVsbG8=", "base64");
console.log("base64 export:", base64Key.export().toString());
console.log("base64 equals hex:", base64Key.equals(hexKey));

const utf8Key = crypto.createSecretKey("68656c6c6f", "utf8");
console.log("utf8 export hex:", utf8Key.export().toString("hex"));
console.log("utf8 size:", utf8Key.symmetricKeySize);
