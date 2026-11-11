import { getDefaultSettings, getPackedSettings } from "node:http2";

console.log(getPackedSettings(getDefaultSettings()).toString("hex"));
