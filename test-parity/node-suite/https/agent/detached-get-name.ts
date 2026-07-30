import * as https from "node:https";

const agent = new https.Agent();
const getName = agent.getName;
console.log(getName({ host: "127.0.0.1", port: 443 }));
agent.destroy();
