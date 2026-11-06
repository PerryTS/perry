import * as crypto from "node:crypto";
import { Buffer } from "node:buffer";

const key = Buffer.from("5333632e722e652e742e4b2e652e5921", "hex");
const iv = Buffer.from("626c616846697a7a3230313142757a7a", "hex");
const plain = Buffer.from("Hello node world!AbC09876dDeFgHi", "latin1");

const padded = crypto.createCipheriv("aes-128-cbc", key, iv);
const paddedCt = Buffer.concat([padded.update(plain), padded.final()]);
console.log("cbc padded len:", paddedCt.length);
console.log("cbc padded hex:", paddedCt.toString("hex"));

const noPad = crypto.createCipheriv("aes-128-cbc", key, iv);
console.log("setAutoPadding returns this:", noPad.setAutoPadding(false) === noPad);
const noPadCt = Buffer.concat([noPad.update(plain), noPad.final()]);
console.log("cbc nopad len:", noPadCt.length);
console.log("cbc nopad hex:", noPadCt.toString("hex"));

const decNoPad = crypto.createDecipheriv("aes-128-cbc", key, iv);
decNoPad.setAutoPadding(false);
const decNoPadPt = Buffer.concat([decNoPad.update(noPadCt), decNoPad.final()]);
console.log("cbc nopad roundtrip:", decNoPadPt.toString("latin1"));

const decPaddedAsRaw = crypto.createDecipheriv("aes-128-cbc", key, iv);
decPaddedAsRaw.setAutoPadding(false);
const rawPadded = Buffer.concat([decPaddedAsRaw.update(paddedCt), decPaddedAsRaw.final()]);
console.log("cbc padded raw len:", rawPadded.length);
console.log("cbc padded raw suffix:", rawPadded.subarray(-16).toString("hex"));
