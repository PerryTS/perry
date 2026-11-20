import * as http from "node:http";
import * as https from "node:https";

const agent = new https.Agent();
agent.addRequest = () => {};
const req = https.request({ agent, host: "example.test" });
req.on("error", () => {});
console.log("ClientRequest:", req instanceof http.ClientRequest);
console.log("method:", req.method);
console.log("protocol:", req.protocol);
console.log("agent:", req.agent instanceof https.Agent);
req.destroy();
agent.destroy();
