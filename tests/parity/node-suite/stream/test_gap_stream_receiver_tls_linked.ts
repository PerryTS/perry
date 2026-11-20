// Web Streams are numeric-id receivers. With node:net / node:tls linked, the
// handle property dispatcher asks the TLS arm first; that arm once boxed the
// stream id as a heap pointer and read a GC header at id - 8 (SIGSEGV in the
// OpenCode TUI: Effect's `hasProperty` did `key in stream`). Every probe
// below must answer like node.
import * as net from "node:net";
import * as tls from "node:tls";

console.log("net linked:", typeof net.createConnection, typeof net.Socket);
console.log("tls linked:", typeof tls.connect, typeof tls.TLSSocket);

function probe(label: string, value: any): void {
  console.log(label, "in x:", "x" in value);
  console.log(label, "in locked:", "locked" in value);
  console.log(label, "read x:", value.x);
  console.log(label, "Predicate.hasProperty:", typeof value === "object" && value !== null && "_tag" in value);
}

const readable = new ReadableStream<string>({
  start(controller) {
    controller.enqueue("a");
    controller.enqueue("b");
    controller.close();
  },
});
probe("ReadableStream", readable);
console.log("ReadableStream locked:", readable.locked);

const writable = new WritableStream<string>({ write() {} });
probe("WritableStream", writable);
console.log("WritableStream locked:", writable.locked);

const transform = new TransformStream<string, string>();
probe("TransformStream", transform);
probe("TransformStream.readable", transform.readable);
probe("TransformStream.writable", transform.writable);

const body = new Response("fetch body").body!;
probe("Response.body", body);
console.log("Response.body locked:", body.locked);

async function main(): Promise<void> {
  const reader = readable.getReader();
  probe("ReadableStreamDefaultReader", reader);
  const chunks: string[] = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
  }
  console.log("ReadableStream chunks:", chunks.join(","));
  console.log("ReadableStream locked after getReader:", readable.locked);

  const writer = writable.getWriter();
  probe("WritableStreamDefaultWriter", writer);
  await writer.write("w");
  await writer.close();
  console.log("WritableStream written");

  const bodyText = await new Response(body).text();
  console.log("Response.body text:", bodyText);
}

main().then(() => console.log("done"));
