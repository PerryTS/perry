import { getPackedSettings } from "node:http2";

console.log("missing:", getPackedSettings().length);
console.log("empty:", getPackedSettings({}).length);
