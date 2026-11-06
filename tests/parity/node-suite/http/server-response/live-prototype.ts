import * as http from "node:http";

const requestProto: any = http.IncomingMessage.prototype;
requestProto.parityHelper = function () { return "request-helper"; };

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
    (request as any).parityHelper(),
  );
  const originalOn = request.on;
  requestProto.on = function () { return "override"; };
  console.log((request as any).on("probe", () => {}));
  requestProto.on = undefined;
  console.log(typeof request.on);
  requestProto.on = originalOn;
  console.log(originalOn.call(request, "aborted", () => {}) === request);
  const custom = { constructor: function Custom() {}, on() { return "custom"; } };
  Object.setPrototypeOf(request, custom);
  console.log(request.constructor === custom.constructor, (request as any).on());
  Object.setPrototypeOf(request, null);
  console.log(typeof request.constructor, typeof request.on, request.method);
  Object.setPrototypeOf(request, requestProto);
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
