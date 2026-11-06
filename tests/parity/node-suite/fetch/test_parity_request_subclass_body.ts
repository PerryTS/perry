const Sub = class extends Request {};
class Req2 extends Request {
  tag: string;
  constructor(input: string, init?: RequestInit) { super(input, init); this.tag = "req2"; }
}
async function main() {
  const sub = new Sub("http://x/y", { method: "POST", body: "hello" });
  console.log("sub.text type:", typeof sub.text);
  console.log("sub instanceof Request:", sub instanceof Request);
  console.log("sub.text body:", await sub.text());
  const r2 = new Req2("http://x/z", { method: "POST", body: '{"n":42}' });
  console.log("r2.tag:", r2.tag);
  console.log("r2 instanceof Request:", r2 instanceof Request);
  console.log("r2.method:", r2.method);
  const j = await r2.json();
  console.log("r2.json.n:", (j as any).n);
}
main();
