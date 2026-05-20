import * as os from "node:os";

const platform = os.platform;
const arch = os.arch;
console.log("captured platform:", typeof platform === "function" && typeof platform() === "string");
console.log("captured arch:", typeof arch === "function" && typeof arch() === "string");
