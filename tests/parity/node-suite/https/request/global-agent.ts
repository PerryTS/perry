import https from "node:https";

const original = https.globalAgent;
const agent = new https.Agent();
agent.addRequest = () => {};
https.globalAgent = agent;
const req = https.request({ host: "example.test" });
req.on("error", () => {});
console.log("request agent:", req.agent === agent);
req.destroy();
agent.destroy();
https.globalAgent = original;
