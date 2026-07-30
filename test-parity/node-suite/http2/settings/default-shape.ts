import { getDefaultSettings } from "node:http2";

const settings = getDefaultSettings();
console.log("null prototype:", Object.getPrototypeOf(settings) === null);
console.log("keys:", Object.keys(settings).join(","));
console.log("values:", Object.values(settings).join(","));
