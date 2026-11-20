import { getUnpackedSettings } from "node:http2";

const bytes = new Uint8Array([0, 4, 0, 0, 0, 7]);
console.log(getUnpackedSettings(bytes).initialWindowSize);
