// `new C(...)` constructs the Web Streams class that C's VALUE is, however the
// program reached it: a renamed import, a const alias of it, a re-export of
// that alias from another module (the @fails-components/webtransport shape:
// `export const ReadableStream = ReadableStreamNode`), a globalThis read, an
// object member, a namespace member or a parameter. Perry used to pick the
// built-in lowering by the written class name, so every form but the first
// built an ordinary object and never ran the source's `start`, and
// `instanceof` against such a value answered false.
import {
  ReadableStream as RN,
  WritableStream as WN,
  TransformStream as TN,
} from "node:stream/web";
import * as web from "node:stream/web";
import { ReadableStream as RX, WritableStream as WX } from "../../../test-files/_helpers/web_stream_ctor_alias.ts";

const RS = RN;
const RG = globalThis.ReadableStream;
const holder = { RS: RN };

function readable(label: string, C: any) {
  let controller: any;
  const stream = new C({
    start(c: any) {
      controller = c;
    },
  });
  console.log(label, typeof controller, stream instanceof ReadableStream, typeof stream.getReader);
  return { stream, controller };
}

async function main() {
  console.log("same constructor", RN === globalThis.ReadableStream, RX === RN, web.ReadableStream === RN);
  readable("renamed", RN);
  readable("const-alias", RS);
  readable("globalThis-alias", RG);
  readable("re-exported-alias", RX);
  readable("member", holder.RS);
  readable("namespace", web.ReadableStream);

  // Inline `new` on each value form, not only through a parameter.
  let c1: any, c2: any, c3: any;
  new RS({ start(c: any) { c1 = c; } });
  new RX({ start(c: any) { c2 = c; } });
  new holder.RS({ start(c: any) { c3 = c; } });
  console.log("inline", typeof c1, typeof c2, typeof c3);

  // The stream works: enqueue through the captured controller, read it back.
  const { stream, controller } = readable("enqueue", RX);
  controller.enqueue("chunk");
  controller.close();
  const reader = stream.getReader();
  console.log("read", JSON.stringify(await reader.read()), JSON.stringify(await reader.read()));

  // Strategy arguments pass through the value path too.
  let desired = -1;
  new RS({ start(c: any) { desired = c.desiredSize; } }, { highWaterMark: 3 });
  console.log("strategy", desired);

  const written: string[] = [];
  const writable = new WX({
    write(chunk: any) {
      written.push(chunk);
    },
  });
  const writer = writable.getWriter();
  await writer.write("a");
  await writer.write("b");
  await writer.close();
  console.log("writable", writable instanceof WritableStream, written.join(","));

  const W = WN;
  try {
    new W(null);
    console.log("null sink: constructed");
  } catch (error) {
    console.log("null sink:", (error as any).code);
  }

  const T = TN;
  const transform = new T({
    transform(chunk: any, c: any) {
      c.enqueue(String(chunk).toUpperCase());
    },
  });
  const tw = transform.writable.getWriter();
  const tr = transform.readable.getReader();
  tw.write("up");
  console.log("transform", transform instanceof TransformStream, JSON.stringify(await tr.read()));

  // `instanceof` against the constructor value answers by the same
  // constructor identity as `new`: a variable, a member, a parameter.
  const isInstance = (value: any, C: any) => value instanceof C;
  const rs = new RS({});
  console.log(
    "instanceof readable",
    rs instanceof RS,
    rs instanceof RX,
    rs instanceof RG,
    rs instanceof holder.RS,
    rs instanceof web.ReadableStream,
    isInstance(rs, RN),
  );
  console.log(
    "instanceof writable",
    writable instanceof W,
    writable instanceof WX,
    writable instanceof web.WritableStream,
    isInstance(writable, WN),
  );
  console.log(
    "instanceof transform",
    transform instanceof T,
    transform instanceof web.TransformStream,
    isInstance(transform, TN),
    isInstance(transform.readable, RS),
    isInstance(transform.writable, W),
  );
  console.log("instanceof renamed import", rs instanceof RN, writable instanceof WN, transform instanceof TN, rs instanceof WN);
  console.log(
    "instanceof other",
    isInstance(rs, WN),
    isInstance(transform, RS),
    isInstance(writable, T),
    isInstance({}, RS),
    isInstance(null, RS),
  );
}

main();
