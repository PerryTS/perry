import http from "node:http";

const agent = http.globalAgent;
const request = { getHeader: () => undefined };

agent.maxSockets = 0;
agent.addRequest(request, "localhost", 8080, "127.0.0.1");
console.log(Object.keys(agent.requests).join(","));

agent.addRequest(request, {
  host: "localhost",
  port: 8080,
  localAddress: "127.0.0.1",
  path: "/ignored",
});
console.log(Object.keys(agent.requests).join(","));
