// Gap test: #10475 — Fetch decodes gzip, deflate, and Brotli response bodies
// while preserving the observable Content-Encoding header. Run against the
// local server from the issue reproduction with PORT set.
const base = `http://127.0.0.1:${process.env.PORT}`;

for (const path of ["/gzip", "/deflate", "/br"]) {
  const response = await fetch(base + path);
  const text = await response.text();
  console.log(path, response.headers.get("content-encoding"), text === '{"compressed":true}');
}

console.log("json", JSON.stringify(await (await fetch(base + "/gzip")).json()));
console.log("accept", await (await fetch(base + "/accept")).text());
console.log(
  "custom",
  await (await fetch(base + "/accept", { headers: { "Accept-Encoding": "identity" } })).text(),
);
