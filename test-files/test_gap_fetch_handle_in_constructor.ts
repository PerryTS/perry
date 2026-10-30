// Web Fetch objects answer `Symbol.iterator in x`, `Symbol.toStringTag in x`
// and `x.constructor` like their prototype chain does, so duck-typing code
// (Headers.fromInput-style `Symbol.iterator in input`) takes the iterable path.
function fromInput(input: any): Record<string, string> {
  if (Symbol.iterator in input) {
    return Object.fromEntries(input as Iterable<[string, string]>);
  }
  return Object.fromEntries(Object.entries(input));
}

const headers = new Headers({ "Content-Type": "text/plain", "X-Id": "7" });
console.log(JSON.stringify(fromInput(headers)));

const form = new FormData();
form.append("k", "v");
const objects: Record<string, any> = {
  Headers: headers,
  FormData: form,
  Request: new Request("http://example.test/"),
  Response: new Response("body"),
  Blob: new Blob(["x"]),
  responseHeaders: new Response("body", { headers: { a: "1" } }).headers,
};
for (const [label, value] of Object.entries(objects)) {
  console.log(
    label,
    Symbol.iterator in value,
    Symbol.asyncIterator in value,
    Symbol.toStringTag in value,
    value.constructor?.name,
  );
}
console.log(objects.Headers.constructor === Headers, objects.Response.constructor === Response);
console.log(JSON.stringify(fromInput(objects.responseHeaders)));
