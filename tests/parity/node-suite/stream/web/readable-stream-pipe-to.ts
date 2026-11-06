import { ReadableStream, WritableStream } from "node:stream/web";
// rs.pipeTo(ws) returns a Promise resolving when piping completes.
const seen: any[] = [];
const rs = new ReadableStream({
  start(c) { c.enqueue("piped"); c.close(); },
});
const ws = new WritableStream({
  write(chunk) { seen.push(chunk); },
});
await rs.pipeTo(ws);
console.log("seen:", seen.join(","));
