import http from "node:http";

let phase = "response";

const server = http.createServer((_request, response) => {
  response.end("x".repeat(2048));
});

server.listen(0, () => {
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("expected a TCP address");
  }

  const request = http.get(`http://127.0.0.1:${address.port}`, (response) => {
    response.on("data", () => {
      console.log("before", request.aborted);
      request.abort();
      phase = "returned";
      console.log("after", request.aborted);
      server.close();
    });

    request.on("abort", () => {
      console.log("abort", phase);
    });
  });
});
