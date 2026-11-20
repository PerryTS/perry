import { Http2ServerResponse } from "node:http2";

for (const method of ["setHeader", "getHeader", "removeHeader"] as const) {
  try {
    (Http2ServerResponse.prototype[method] as any).call({}, "x-test", "yes");
  } catch (error: any) {
    console.log(method, error.name, error.code);
  }
}
