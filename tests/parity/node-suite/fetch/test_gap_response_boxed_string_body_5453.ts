// new Response(<boxed String object>) must ToString-coerce the body (#5453).
// hono's raw() / hono/html / JSX c.html() return `new String(value)` with an
// `isEscaped` expando — a heap OBJECT, not a primitive string. Before the fix
// the body coercion handed js_response_new that object's raw address, which it
// read as a StringHeader: the bogus byte_len triggered a multi-GB memmove and
// SIGSEGV'd on the first server-rendered page.
// Expected output:
// len=20013
// head=<html>
// status=200
// boxed-short=hello

async function main(): Promise<void> {
  const big = "<html>" + "x".repeat(20000) + "</html>";
  const boxed: any = new String(big);
  boxed.isEscaped = true;
  const r = new Response(boxed as BodyInit, {
    status: 200,
    headers: { "content-type": "text/html" },
  });
  const txt = await r.text();
  console.log("len=" + txt.length);
  console.log("head=" + txt.slice(0, 6));
  console.log("status=" + r.status);

  // Short boxed String must also coerce to its primitive.
  const r2 = new Response(new String("hello") as BodyInit);
  console.log("boxed-short=" + (await r2.text()));
}
void main();
