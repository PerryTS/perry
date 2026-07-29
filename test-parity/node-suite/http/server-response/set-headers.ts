import { IncomingMessage, ServerResponse } from "node:http";

for (
  const headers of [
    new Map([["X-One", "one"], ["X-Two", ["two", "three"]]]),
    new Headers({ "X-Three": "three" }),
  ] as any[]
) {
  const response = new ServerResponse(new IncomingMessage(null as any));
  console.log("self:", response.setHeaders(headers) === response);
  console.log(
    "values:",
    JSON.stringify(
      Object.entries(response.getHeaders()).sort(([a], [b]) =>
        a.localeCompare(b)
      ),
    ),
  );
}

try {
  new ServerResponse(new IncomingMessage(null as any)).setHeaders(
    { "X-One": "one" } as any,
  );
} catch (error: any) {
  console.log("object:", error.name, error.code);
}
