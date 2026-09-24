// Exceed the entire shared Fetch handle band using constructors that allocate
// no Perry heap payload. Registry pressure must collect and recycle ids while
// preserving retained owners, their child handles, and subclass wrappers.
declare function gc(): void;

class RetainedResponse extends Response {}

async function main() {
  const request = new Request("https://example.com/retained", {
    method: "POST", body: "request body", headers: { "x-request": "yes" },
  });
  const response = new Response("response body", {
    status: 201, headers: { "x-response": "yes" },
  });
  const headers = response.headers;
  const getter = headers.get.bind(headers);
  const form = new FormData();
  form.append("file", new Blob(["file body"], { type: "text/plain" }), "part.txt");
  const subclass = new RetainedResponse("subclass body", { status: 202 });
  for (let i = 0; i < 700000; i++) {
    const temporary = new Headers();
    if (temporary.has("unexpected")) throw new Error("fresh Headers must be empty");
  }
  if (typeof gc === "function") gc();
  console.log(request.method, request.headers.get("x-request"), request.signal.aborted);
  console.log(response.status, response.headers === headers, getter("x-response"));
  console.log(subclass.status, subclass instanceof Response);
  console.log(await request.text(), await response.text(), await subclass.text());
  const file = form.get("file") as File;
  console.log(file.name, file.type, await file.text());
}
main();
