// #2638: Response.json(data, init) must honor init.status / init.statusText /
// init.headers. Defaults: status 200, statusText "" (Node does NOT derive it
// from the status code for this factory), content-type application/json (unless
// the init headers already set one).
async function main() {
  // Default: no init.
  const r1 = Response.json({ a: 1 });
  console.log(r1.status, r1.statusText, r1.headers.get("content-type"), r1.headers.get("x-test"));
  console.log(await r1.json());

  // status only.
  const r2 = Response.json({ a: 1 }, { status: 404 });
  console.log(r2.status, r2.statusText, r2.headers.get("content-type"), r2.headers.get("x-test"));

  // status + statusText.
  const r3 = Response.json({ a: 1 }, { status: 201, statusText: "Created" });
  console.log(r3.status, r3.statusText, r3.headers.get("content-type"), r3.headers.get("x-test"));

  // statusText only (status stays 200).
  const r4 = Response.json({ a: 1 }, { statusText: "Custom" });
  console.log(r4.status, r4.statusText, r4.headers.get("content-type"), r4.headers.get("x-test"));

  // All three including an extra header.
  const r5 = Response.json({ a: 1 }, { status: 404, statusText: "Nope", headers: { "x-test": "yes" } });
  console.log(r5.status, r5.statusText, r5.headers.get("content-type"), r5.headers.get("x-test"));
  console.log(await r5.json());
}
main();
