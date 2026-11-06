import http from "node:http";

let finishFirstResponse: (() => void) | undefined;
const server = http.createServer((request, response) => {
  if (request.url !== "/") {
    throw new Error(`queued request reached server: ${request.url}`);
  }
  response.write("ready");
  finishFirstResponse = () => response.end();
});

server.listen(0, () => {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("expected a TCP address");
  }

  const agent = new http.Agent({ maxSockets: 1 });
  console.log("initial", Object.keys(agent.sockets).length);
  const first = http.request({
    host: "127.0.0.1",
    port: address.port,
    path: "/",
    agent,
  });
  first.end();

  first.on("response", (response) => {
    console.log(
      "active",
      Object.keys(agent.sockets).length,
      Object.keys(agent.requests).length,
    );
    const queued = http.request({
      host: "127.0.0.1",
      port: address.port,
      path: "/queued",
      agent,
    });
    queued.end();
    console.log(
      "queued",
      Object.keys(agent.sockets).length,
      Object.keys(agent.requests).length,
    );
    queued.abort();
    console.log(
      "aborted",
      Object.keys(agent.sockets).length,
      Object.keys(agent.requests).length,
    );

    response.on("data", () => finishFirstResponse?.());
    response.on("end", () => {
      setTimeout(() => {
        console.log(
          "released",
          Object.keys(agent.sockets).length,
          Object.keys(agent.requests).length,
        );
        agent.destroy();
        server.close();
      }, 20);
    });
  });
});
