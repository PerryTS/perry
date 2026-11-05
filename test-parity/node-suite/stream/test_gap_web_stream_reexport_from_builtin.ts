// `export { X } from "node:stream/web"` in a user module forwards the
// builtin's export: importers get the constructor itself (one identity with
// globalThis.ReadableStream), and the program links. Perry used to keep the
// form as a plain re-export of a module it never compiles, so importers
// referenced a `__perry_wrap_perry_fn_<module>__ReadableStream` wrapper that
// no module defines and the link failed.
import {
  ReadableStream,
  WritableStream as RenamedWritable,
  TransformStream,
} from "../../../test-files/_helpers/web_stream_reexport_from.ts";
import { ReadableStream as Twice } from "../../../test-files/_helpers/web_stream_reexport_twice.ts";

console.log(
  "identity",
  ReadableStream === globalThis.ReadableStream,
  RenamedWritable === globalThis.WritableStream,
  TransformStream === globalThis.TransformStream,
  Twice === globalThis.ReadableStream,
);
console.log("typeof", typeof ReadableStream, typeof RenamedWritable, typeof Twice);

let controller: any;
const stream = new ReadableStream({
  start(c: any) {
    controller = c;
  },
});
let sink: any;
new RenamedWritable({
  start(c: any) {
    sink = c;
  },
});
let again: any;
new Twice({
  start(c: any) {
    again = c;
  },
});
console.log("started", typeof controller, typeof sink, typeof again, typeof stream.getReader);
