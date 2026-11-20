const init = {
  status: 201,
  statusText: "Created",
  headers: { "content-type": "application/x-www-form-urlencoded" },
};

const res = new Response("a=1&a=2&b=two", init);
console.log("response status:", res.status);
console.log("response statusText:", JSON.stringify(res.statusText));
console.log("response ok:", res.ok);
console.log("response type:", res.type);
console.log("response url:", JSON.stringify(res.url));
console.log("response redirected:", res.redirected);
console.log("response bytes typeof:", typeof res.bytes);
console.log("response formData typeof:", typeof res.formData);

const bytesRes = new Response("a=1&a=2&b=two", init);
const bytes = await bytesRes.bytes();
console.log("bytes length:", bytes.length);
console.log("bytes first:", bytes[0]);
console.log("bytes last:", bytes[bytes.length - 1]);

const formRes = new Response("a=1&a=2&b=two", init);
const form = await formRes.formData();
console.log("form get a:", form.get("a"));
console.log("form missing:", form.get("missing") === null);
console.log("form getAll a:", JSON.stringify(form.getAll("a")));
console.log("form entries:", JSON.stringify(Array.from(form.entries())));

console.log("Response.error typeof:", typeof Response.error);
const err = Response.error();
console.log("error status:", err.status);
console.log("error ok:", err.ok);
console.log("error type:", err.type);
console.log("error statusText:", JSON.stringify(err.statusText));
console.log("error url:", JSON.stringify(err.url));
console.log("error redirected:", err.redirected);
console.log("error bodyUsed before:", err.bodyUsed);
console.log("error headers:", JSON.stringify(Array.from(err.headers.entries())));
console.log("error text:", JSON.stringify(await err.text()));
console.log("error bodyUsed after:", err.bodyUsed);
