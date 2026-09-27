// Request/Response bodies built from async iterables (async generators),
// the shape Astro's node adapter uses for POST bodies.
const enc = new TextEncoder();

async function* bytes(...parts: string[]) {
  for (const p of parts) yield enc.encode(p);
}

async function* strings(...parts: string[]) {
  for (const p of parts) yield p;
}

async function* empty() {}

async function* throwing() {
  yield enc.encode("partial");
  throw new Error("boom");
}

function req(body: unknown): Request {
  return new Request("http://x/", { method: "POST", body, duplex: "half" } as any);
}

async function attempt(label: string, fn: () => Promise<unknown>) {
  try {
    console.log(label, JSON.stringify(await fn()));
  } catch (e: any) {
    console.log(label, "threw", e?.name, e?.message);
  }
}

await attempt("text:", () => req(bytes('{"a":', "1}")).text());
await attempt("json:", () => req(bytes('{"a":', "1}")).json());
await attempt("arrayBuffer:", async () => {
  const buf = await req(bytes("ab", "cd")).arrayBuffer();
  return [buf.byteLength, ...new Uint8Array(buf)];
});
await attempt("bodyUsed:", async () => {
  const r = req(bytes("x"));
  const before = r.bodyUsed;
  await r.text();
  return [before, r.bodyUsed];
});
await attempt("string chunks:", () => req(strings("a", "b")).text());
await attempt("empty:", () => req(empty()).text());
await attempt("throwing:", () => req(throwing()).text());
await attempt("response:", () => new Response(bytes("res", "ponse") as any).text());
await attempt("response json:", () => new Response(bytes('[1,', "2]") as any).json());
