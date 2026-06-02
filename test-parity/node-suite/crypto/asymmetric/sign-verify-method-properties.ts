import * as crypto from "node:crypto";

const sign = crypto.createSign("sha256");
console.log("sign update name:", sign.update.name);
console.log("sign sign name:", sign.sign.name);
console.log("sign update typeof:", typeof sign.update);
console.log("sign sign typeof:", typeof sign.sign);

const verify = crypto.createVerify("sha256");
console.log("verify update name:", verify.update.name);
console.log("verify verify name:", verify.verify.name);
console.log("verify update typeof:", typeof verify.update);
console.log("verify verify typeof:", typeof verify.verify);
