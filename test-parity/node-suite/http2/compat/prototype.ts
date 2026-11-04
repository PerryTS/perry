// #13802: the compat API's `req`/`res` inherit from
// `Http2ServerRequest.prototype` / `Http2ServerResponse.prototype`.
import * as http2 from "node:http2";

// A framework-style helper patched onto the prototype reaches `res`.
const responseProto: any = http2.Http2ServerResponse.prototype;
responseProto.sendText = function (this: any, text: string) {
  this.setHeader("content-type", "text/plain");
  this.end(text);
};

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
      );
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
