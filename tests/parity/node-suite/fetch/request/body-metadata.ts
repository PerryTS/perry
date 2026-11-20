const init = {
  method: "post",
  body: "a=1&a=2&b=two",
  headers: { "content-type": "application/x-www-form-urlencoded" },
  referrer: "https://referrer.test/a",
  referrerPolicy: "origin",
  mode: "cors",
  credentials: "include",
  cache: "no-store",
  redirect: "manual",
  integrity: "sha256-abc",
  keepalive: true,
  duplex: "half",
};

const req = new Request("https://example.test/path", init);

for (const key of [
  "url",
  "method",
  "destination",
  "referrer",
  "referrerPolicy",
  "mode",
  "credentials",
  "cache",
  "redirect",
  "integrity",
  "duplex",
]) {
  console.log(`${key}:`, JSON.stringify(req[key]));
}
console.log("keepalive:", req.keepalive);
console.log("signal typeof:", typeof req.signal);
console.log("signal aborted:", req.signal.aborted);
console.log("headers content-type:", req.headers.get("content-type"));

console.log("request blob typeof:", typeof req.blob);
console.log("request bytes typeof:", typeof req.bytes);
console.log("request formData typeof:", typeof req.formData);

const blobReq = new Request("https://example.test/body", init);
const blob = await blobReq.blob();
console.log("blob size:", blob.size);
console.log("blob type:", blob.type);
console.log("blob text:", await blob.text());

const bytesReq = new Request("https://example.test/body", init);
const bytes = await bytesReq.bytes();
console.log("bytes length:", bytes.length);
console.log("bytes first:", bytes[0]);
console.log("bytes last:", bytes[bytes.length - 1]);

const formReq = new Request("https://example.test/body", init);
const form = await formReq.formData();
console.log("form get a:", form.get("a"));
console.log("form missing:", form.get("missing") === null);
console.log("form getAll a:", JSON.stringify(form.getAll("a")));
console.log("form entries:", JSON.stringify(Array.from(form.entries())));
