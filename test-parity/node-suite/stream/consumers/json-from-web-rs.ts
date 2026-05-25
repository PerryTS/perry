import { ReadableStream } from "node:stream/web";
import { json } from "node:stream/consumers";
// json() works on Web ReadableStream.
const rs = new ReadableStream({
  start(c) { c.enqueue(`{"ok":`); c.enqueue(`true}`); c.close(); },
});
const result = await json(rs as any) as any;
console.log("ok:", result.ok);
