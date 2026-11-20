import { getPackedSettings } from "node:http2";

console.log(
  getPackedSettings({ customSettings: { 9999: 301 } }).toString("hex"),
);
