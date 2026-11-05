import * as http2 from "node:http2";

const responseProto: any = http2.Http2ServerResponse.prototype;
responseProto.sendText = function (this: any, text: string) {
  this.setHeader("content-type", "text/plain");
  this.end(text);
};

const requestProto: any = http2.Http2ServerRequest.prototype;
requestProto.parityHelper = function () { return "request-helper"; };
const server = http2.createServer();
let client: any;
try {
  const received = new Promise<void>((resolve) => {
    server.on("request", (request: any, response: any) => {
      console.log(
        Object.getPrototypeOf(response) === http2.Http2ServerResponse.prototype,
        response instanceof http2.Http2ServerResponse,
        Object.getPrototypeOf(request) === http2.Http2ServerRequest.prototype,
        request instanceof http2.Http2ServerRequest,
      );
      console.log(
        response.constructor === http2.Http2ServerResponse,
        response.constructor.name,
        request.constructor === http2.Http2ServerRequest,
        typeof response.sendText,
        request.parityHelper(),
      );
      const originalOn = request.on;
      requestProto.on = function () { return "override"; };
      console.log(request.on("probe", () => {}));
      requestProto.on = undefined;
      console.log(typeof request.on);
      requestProto.on = originalOn;
      console.log(originalOn.call(request, "aborted", () => {}) === request);
      const custom = { constructor: function Custom() {}, on() { return "custom"; } };
      Object.setPrototypeOf(request, custom);
      console.log(request.constructor === custom.constructor, request.on());
      Object.setPrototypeOf(request, null);
      console.log(typeof request.constructor, typeof request.on, request.method);
      Object.setPrototypeOf(request, new Proxy({}, {
        get(_target, key, receiver) { return key === "parityProxy" && receiver === request; },
        has() { throw new Error("unexpected has trap"); },
      }));
      console.log(request.parityProxy);
      Object.setPrototypeOf(request, requestProto);
      const responseCustom = { constructor: function CustomResponse() {} };
      Object.setPrototypeOf(response, responseCustom);
      console.log(response.constructor === responseCustom.constructor, response.statusCode, response.req === request, response.writable);
      Object.setPrototypeOf(response, null);
      console.log(typeof response.constructor, typeof response.end, response.statusCode, response.req === request, response.writable);
      Object.setPrototypeOf(response, responseProto);
      response.sendText("ok");
      resolve();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const completed = new Promise<void>((resolve, reject) => {
    const request = client.request({ ":path": "/" });
    request.on("response", (headers: any) => console.log(headers["content-type"]));
    request.setEncoding("utf8");
    request.on("data", (chunk: string) => console.log(chunk));
    request.on("error", reject);
    request.on("end", resolve);
    request.end();
  });
  await Promise.all([received, completed]);
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
