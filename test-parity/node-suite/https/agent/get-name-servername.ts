import * as https from "node:https";

const agent = new https.Agent();
const base = { host: "example.test", port: 443 };
console.log("omitted:", agent.getName(base));
console.log(
  "same host:",
  agent.getName({ ...base, servername: "example.test" }),
);
console.log(
  "different host:",
  agent.getName({ ...base, servername: "sni.test" }),
);
agent.destroy();
