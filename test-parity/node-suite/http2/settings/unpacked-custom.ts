import { Buffer } from "node:buffer";
import { getUnpackedSettings } from "node:http2";

const settings = getUnpackedSettings(Buffer.from("270f0000012d", "hex"));
console.log(JSON.stringify(settings));
