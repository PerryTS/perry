import { Http2ServerRequest, Http2ServerResponse } from "node:http2";
if (typeof Http2ServerRequest !== "function") throw new Error("Http2ServerRequest type");
if (typeof Http2ServerResponse !== "function") throw new Error("Http2ServerResponse type");
const someObj: any = { url: "/x" };
if (someObj instanceof Http2ServerRequest) throw new Error("plain object matched request");
if ((42 as any) instanceof Http2ServerResponse) throw new Error("number matched response");
console.log("OK");
