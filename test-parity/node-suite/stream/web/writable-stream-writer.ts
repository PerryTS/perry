import { WritableStream } from "node:stream/web";
// writableStream.getWriter() returns a writer; writer.write() / writer.close()
// drives the stream.
const seen: any[] = [];
const ws = new WritableStream({
  write(chunk) { seen.push(chunk); },
});
const writer = ws.getWriter();
await writer.write("alpha");
await writer.close();
console.log("seen:", seen.join(","));
