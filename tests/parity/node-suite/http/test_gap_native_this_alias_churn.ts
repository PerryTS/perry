// Native-this aliases (#4973 `http.Server.call(this, ...)`, #10454
// `http.ServerResponse.call(this, req)`) live on the aliasing object. A churn
// of light-my-request-style responses (fastify `inject` builds one per
// request) must leave every survivor still forwarding to its own native
// response after collections have moved it, chainable methods must return
// the object itself, and the classic server subclass must still listen.
import * as http from "node:http";
import * as util from "node:util";

const H: any = http;

function Response(this: any, req: any, tag: number) {
  H.ServerResponse.call(this, req);
  this.tag = tag;
  this.payload = new Array(64).fill(tag);
}
util.inherits(Response as any, H.ServerResponse);

const kept: any[] = [];
let checksum = 0;
for (let i = 0; i < 3000; i++) {
  const r = new (Response as any)({ method: "GET" }, i);
  const chained = r.setHeader("x-tag", String(i));
  if (chained !== r) console.log("setHeader did not return this at", i);
  checksum = (checksum + r.payload.length + Number(r.getHeader("x-tag"))) % 1000003;
  if (i % 500 === 0) kept.push(r);
}
// Survivors read their own native state after the churn.
for (const r of kept) {
  console.log(
    r.tag,
    r.getHeader("x-tag"),
    r.hasHeader("x-tag"),
    r.statusCode,
    r instanceof H.ServerResponse,
    typeof r.end,
  );
}
kept[1].removeHeader("x-tag");
console.log("removed", kept[1].hasHeader("x-tag"), "other", kept[2].hasHeader("x-tag"));
console.log("checksum", checksum);

// #4973: the pre-class server subclass still behaves as the server.
function TestServer(this: any) {
  H.Server.call(this, (_req: any, res: any) => res.end("ok"));
}
Object.setPrototypeOf((TestServer as any).prototype, H.Server.prototype);
const server = new (TestServer as any)();
console.log("server listen", typeof server.listen, "close", typeof server.close);
server.listen(0, () => {
  const port = server.address().port;
  console.log("listening", typeof port === "number" && port > 0);
  server.close(() => console.log("closed"));
});
