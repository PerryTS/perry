// The http module's tables and validators, and the fetch classes, link and run
// on the core-module target without any socket (net itself is unsupported there).
import * as http from "node:http";

console.log(http.STATUS_CODES[404], http.STATUS_CODES[204], http.STATUS_CODES[503]);
console.log(http.METHODS.includes("GET"), http.METHODS.includes("PATCH"), http.METHODS.length > 20);
for (const name of ["x-ok", "bad name"]) {
  try {
    http.validateHeaderName(name);
    console.log("header ok", name);
  } catch (e: any) {
    console.log("header rejected", name, e.code);
  }
}

const headers = new Headers({ "Content-Type": "text/plain", "X-Id": "7" });
headers.append("x-id", "8");
console.log(headers.get("content-type"), headers.get("x-id"), headers.has("missing"));

const request = new Request("http://example.test/a?b=1", { method: "POST", body: "payload" });
console.log(request.method, new URL(request.url).search);

async function main() {
  console.log(await request.text());
  const response = new Response('{"n":3}', { status: 201, headers: { "content-type": "application/json" } });
  console.log(response.status, response.ok, (await response.json()).n);
}
main();
