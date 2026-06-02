// #3654: node:http2 exposes the settings helper trio
// `getDefaultSettings`, `getPackedSettings`, and `getUnpackedSettings`
// alongside the server factories. Lock in Node's observable shape and
// round-trip behavior so the export surface can't silently regress.
import * as http2 from "node:http2";
import { Buffer } from "node:buffer";

console.log("typeof getDefaultSettings:", typeof http2.getDefaultSettings);
console.log("typeof getPackedSettings:", typeof http2.getPackedSettings);
console.log("typeof getUnpackedSettings:", typeof http2.getUnpackedSettings);

const defaults = http2.getDefaultSettings();
console.log("defaults type:", typeof defaults);
console.log("enablePush:", defaults.enablePush);
console.log("initialWindowSize:", defaults.initialWindowSize);

const packed = http2.getPackedSettings({ enablePush: false, initialWindowSize: 1024 });
console.log("packed is Buffer:", Buffer.isBuffer(packed));

const unpacked = http2.getUnpackedSettings(packed);
console.log("round-trip enablePush:", unpacked.enablePush);
console.log("round-trip initialWindowSize:", unpacked.initialWindowSize);
