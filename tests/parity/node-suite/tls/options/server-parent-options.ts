import tls from "node:tls";

const defaults = tls.createServer();
console.log("defaults:", (defaults as any).allowHalfOpen, (defaults as any).pauseOnConnect);
const configured = tls.createServer({ allowHalfOpen: true, pauseOnConnect: true });
console.log("configured:", (configured as any).allowHalfOpen, (configured as any).pauseOnConnect);
console.log("instances:", defaults instanceof tls.Server, configured instanceof tls.Server);
