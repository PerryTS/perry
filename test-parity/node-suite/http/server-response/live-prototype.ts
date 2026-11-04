// #13802: a live server's `req`/`res` inherit from
// `IncomingMessage.prototype` / `ServerResponse.prototype`.
import * as http from "node:http";

const server = http.createServer((request, response) => {
  console.log(
    Object.getPrototypeOf(response) === http.ServerResponse.prototype,
    response instanceof http.ServerResponse,
    Object.getPrototypeOf(request) === http.IncomingMessage.prototype,
    request instanceof http.IncomingMessage,
  );
  console.log(
    response.constructor === http.ServerResponse,
    response.constructor.name,
    request.constructor === http.IncomingMessage,
  );
  response.end("ok");
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
try {
  await new Promise<void>((resolve, reject) => {
    http
      .get(`http://127.0.0.1:${(server.address() as any).port}/`, (response) => {
        response.resume();
        response.on("end", resolve);
      })
      .on("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
