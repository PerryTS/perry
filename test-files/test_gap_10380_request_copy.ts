// #10380: wrapping a Request must inherit fields unless init overrides them.
function source(): Request {
  return new Request("https://example.test/session", {
    method: "POST",
    body: "payload",
    headers: { "x-original": "yes" },
    credentials: "include",
    cache: "no-store",
    redirect: "manual",
  });
}
const plain = new Request(source());
console.log("plain", plain.method, plain.url, await plain.text());
console.log("metadata", plain.credentials, plain.cache, plain.redirect);
const wrapped = new Request(source(), { headers: { authorization: "Bearer test" } });
console.log("wrapped", wrapped.method, await wrapped.text());
console.log("headers", wrapped.headers.get("authorization"), wrapped.headers.get("x-original"));
function options(): RequestInit {
  return { method: "PUT", body: "replacement" };
}
const overridden = new Request(source(), options());
console.log("override", overridden.method, await overridden.text());
console.log("inherited header", overridden.headers.get("x-original"));
const bytes = new Request("https://example.test/bytes", { method: "POST", body: new Uint8Array([0, 127, 255]) });
const copiedBytes = new Uint8Array(await new Request(bytes, {}).arrayBuffer());
console.log("bytes", copiedBytes[0], copiedBytes[1], copiedBytes[2]);
const original = source();
original.headers.set("x-later", "updated");
const inherited = new Request(original);
console.log("mutated headers", inherited.headers.get("x-later"));
console.log("body transfer", original.bodyUsed, inherited.bodyUsed);
console.log("transferred text", await inherited.text());
const consumed = source();
await consumed.text();
try {
  new Request(consumed);
  console.log("used body accepted");
} catch (e) {
  console.log("used body rejected", e instanceof TypeError);
}
try {
  new Request(source(), { method: "GET" });
  console.log("GET body accepted");
} catch (e) {
  console.log("GET body rejected", e instanceof TypeError);
}
const nullBody = new Request(source(), { body: null });
console.log("null inherits", nullBody.method, await nullBody.text());
const RequestCtor: any = globalThis.Request;
function construct(C: any, input: any, init: any): any { return new C(input, init); }
const reflective = construct(RequestCtor, source(), { headers: { "x-reflective": "yes" } });
console.log("reflective", reflective.method, await reflective.text(), reflective.headers.get("x-reflective"));
const stream = new ReadableStream({
  start(controller) {
    controller.enqueue(new Uint8Array([65, 66]));
    controller.close();
  },
});
const streamed = new RequestCtor("https://example.test/stream", { method: "POST", body: stream, duplex: "half" });
console.log("reflective stream", await streamed.text());

class DerivedRequest extends Request {}
const derived = new DerivedRequest("https://example.test/derived", {
  method: "PUT", body: "derived-body", headers: { "x-derived": "yes" },
});
const derivedCopy = new Request(derived);
console.log("subclass", derivedCopy.method, await derivedCopy.text(), derivedCopy.headers.get("x-derived"));
console.log("subclass transfer", derived.bodyUsed);
try {
  new Request(derived);
  console.log("subclass reused");
} catch (e) {
  console.log("subclass used rejected", e instanceof TypeError);
}
const textBody = new Request("https://example.test/text", { method: "POST", body: "text" });
console.log("text content type", textBody.headers.get("content-type"));
const explicitType = new Request("https://example.test/text", {
  method: "POST", body: "text", headers: { "content-type": "application/custom" },
});
console.log("explicit content type", explicitType.headers.get("content-type"));
const noBody = new Request("https://example.test/empty");
console.log("empty content type", noBody.headers.get("content-type"));
// A Headers handle used as a body used to be dereferenced as a StringHeader.
// This pins safe construction; coercion of its payload is a separate gap.
const handleBody = new Request(source(), { body: new Headers() as any });
console.log("handle body constructed", handleBody.method);
const overrideStream = new ReadableStream({
  start(controller) { controller.enqueue(new Uint8Array([67, 68])); controller.close(); },
});
const streamCopy = new Request(source(), { body: overrideStream, duplex: "half" });
console.log("stream override", await streamCopy.text());
