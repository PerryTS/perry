import { getPackedSettings } from "node:http2";

console.log("missing:", getPackedSettings().toString("hex"));
console.log("empty:", getPackedSettings({}).toString("hex"));
