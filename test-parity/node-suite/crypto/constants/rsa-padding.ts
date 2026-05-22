import crypto from "node:crypto";
// crypto.constants exposes the OpenSSL-defined numeric padding/SSL constants.
console.log("RSA_PKCS1_PADDING:", typeof crypto.constants.RSA_PKCS1_PADDING);
console.log("RSA_PKCS1_OAEP_PADDING:", typeof crypto.constants.RSA_PKCS1_OAEP_PADDING);
