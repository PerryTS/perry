import tls from "node:tls";

const defaults = tls.createServer();
console.log("defaults:", defaults.allowHalfOpen, defaults.pauseOnConnect);
const configured = tls.createServer({ allowHalfOpen: true, pauseOnConnect: true });
console.log("configured:", configured.allowHalfOpen, configured.pauseOnConnect);
console.log("instances:", defaults instanceof tls.Server, configured instanceof tls.Server);
