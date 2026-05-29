import querystring from "node:querystring";

const unescapeBuffer = (querystring as any).unescapeBuffer;

console.log("keys:", Object.keys(querystring).sort().join(","));
console.log("typeof:", typeof unescapeBuffer, unescapeBuffer.name);

const decoded = unescapeBuffer("a%20b+%E2%9C%93", true);
console.log("decoded:", Buffer.isBuffer(decoded), decoded.toString("utf8"));
console.log("decoded bytes:", Array.from(decoded).join("-"));

const plus = unescapeBuffer("a+b", false);
console.log("plus preserved:", plus.toString("utf8"), Array.from(plus).join("-"));

const malformed = unescapeBuffer("abc%zzdef", true);
console.log("malformed:", malformed.toString("utf8"), Array.from(malformed).join("-"));
