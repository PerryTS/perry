import http from "node:http";

const agent = new http.Agent({ keepAlive: true, keepAliveMsecs: 1250 });
const originalCreateConnection = agent.createConnection;

agent.createConnection = function (options: any, ...args: any[]) {
  console.log(options.keepAlive);
  console.log(options.keepAliveInitialDelay);
  return originalCreateConnection.call(agent, options, ...args);
};

const server = http.createServer((_request, response) => response.end("ok"));

server.listen(0, () => {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("expected a TCP address");
  }
  http.get(
    { host: "127.0.0.1", port: address.port, agent },
    (response) => response.on("end", () => server.close()).resume(),
  );
});
