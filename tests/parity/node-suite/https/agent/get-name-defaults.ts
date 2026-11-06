import * as https from "node:https";

const agent = new https.Agent();
console.log(agent.getName({ host: "example.test", port: 443 }));
agent.destroy();
