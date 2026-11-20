const GlobalRequest = (globalThis as any).Request;
class Req extends GlobalRequest {
  constructor(input: any, init?: any) { super(input, init); }
}
async function main() {
  const r = new Req("http://x/y", { method: "POST", body: "HI" });
  console.log("typeof r.text:", typeof r.text);
  const k = "text";
  console.log("typeof r[k]:", typeof (r as any)[k]);
  console.log("literal text:", await r.text());
  const r2 = new Req("http://x/z", {
    method: "POST",
    body: new ReadableStream({
      start(c: any) { c.enqueue(new TextEncoder().encode("STREAMED")); c.close(); },
    }),
  });
  const fn = (r2 as any)["text"];
  console.log("computed stream text:", await fn.call(r2));
}
main();
