import { getDefaultSettings } from "node:http2";

const first = getDefaultSettings();
first.headerTableSize = 1;
const second = getDefaultSettings();
console.log("distinct:", first !== second);
console.log("fresh:", second.headerTableSize);
