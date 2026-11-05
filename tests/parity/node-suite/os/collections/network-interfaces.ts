import * as os from "node:os";

const interfaces = os.networkInterfaces();
console.log("interfaces object:", interfaces !== null && typeof interfaces === "object");
console.log("interfaces not array:", !Array.isArray(interfaces));
