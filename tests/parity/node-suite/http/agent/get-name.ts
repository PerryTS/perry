import { Agent } from "node:http";

const agent = new Agent();
console.log("host:", agent.getName({ host: "example.test" }));
console.log(
  "port/family:",
  agent.getName({ host: "example.test", port: 8080, family: 4 }),
);
console.log(
  "local address:",
  agent.getName({ host: "example.test", localAddress: "127.0.0.2" }),
);
console.log(
  "socket path ignored:",
  agent.getName({ host: "example.test", socketPath: "/tmp/http.sock" }),
);
console.log("empty:", agent.getName());
agent.destroy();
