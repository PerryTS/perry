async function main() {
  const bin = new Uint8Array([0xff, 0xfe, 0x00, 0x80, 0x50, 0x4e, 0x47]);
  const ab = await new Blob([bin]).arrayBuffer();
  console.log("blob.arrayBuffer:", Array.from(new Uint8Array(ab)).join(","));
  const res = new Response('{"msg":"héllo","n":42}');
  console.log("res.text:", await res.text());
  const json = await new Response('{"msg":"héllo","n":42}').json();
  console.log("res.json:", (json as any).msg, (json as any).n);
  const ab2 = await new Response("ABC").arrayBuffer();
  console.log("res.arrayBuffer:", Array.from(new Uint8Array(ab2)).join(","));
}
main().catch((e) => console.log("ERR:", e?.message ?? e));
