import { Http2ServerRequest, Http2ServerResponse } from "node:http2";

for (
  const [name, prototype, keys] of [
    ["request", Http2ServerRequest.prototype, ["method", "url", "headers"]],
    ["response", Http2ServerResponse.prototype, [
      "statusCode",
      "headersSent",
      "sendDate",
    ]],
  ] as const
) {
  for (const key of keys) {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, key);
    console.log(
      name,
      key,
      typeof descriptor?.get,
      typeof descriptor?.set,
      descriptor?.enumerable,
      descriptor?.configurable,
    );
  }
}
