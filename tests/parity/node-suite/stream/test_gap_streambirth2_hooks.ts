import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";
const hooks: any = {
  read() {},
  write(_chunk: any, _encoding: any, cb: any) { cb(); },
  writev(_chunks: any, cb: any) { cb(); },
  destroy(err: any, cb: any) { cb(err); },
  final(cb: any) { cb(); },
  construct(cb: any) { cb(); },
  transform(chunk: any, _encoding: any, cb: any) { cb(null, chunk); },
  flush(cb: any) { cb(); },
};
const names = ["read", "write", "writev", "destroy", "final", "construct", "transform", "flush"];
function inspect(label: string, stream: any) {
  console.log(label, Object.keys(stream).join(","), Object.getOwnPropertyNames(stream).join(","));
  if (stream._readableState) console.log("r", Object.keys(stream._readableState).join(","), Object.getOwnPropertyNames(stream._readableState).join(","));
  if (stream._writableState) console.log("w", Object.keys(stream._writableState).join(","), Object.getOwnPropertyNames(stream._writableState).join(","));
}
class R extends Readable { constructor(o: any) { super(o); } }
class W extends Writable { constructor(o: any) { super(o); } }
class D extends Duplex { constructor(o: any) { super(o); } }
for (let bits = 0; bits < 256; bits++) {
  const opts: any = {};
  for (let i = 0; i < names.length; i++) if ((bits & (1 << i)) !== 0) opts[names[i]] = hooks[names[i]];
  inspect("R:" + bits, new Readable(opts));
  inspect("W:" + bits, new Writable(opts));
  inspect("D:" + bits, new Duplex(opts));
  inspect("T:" + bits, new Transform(opts));
  inspect("P:" + bits, new PassThrough(opts));
  inspect("r:" + bits, new R(opts));
  inspect("w:" + bits, new W(opts));
  inspect("d:" + bits, new D(opts));
}

// Values can change while the options object retains its ShapeId.
const changing: any = { read: undefined, destroy: undefined };
for (let i = 0; i < 16; i++) new Readable(changing);
inspect("warm-none", new Readable(changing));
changing.read = hooks.read;
inspect("warm-read", new Readable(changing));
changing.destroy = hooks.destroy;
inspect("warm-read-destroy", new Readable(changing));
changing.read = undefined;
inspect("warm-destroy", new Readable(changing));
changing.destroy = undefined;
inspect("warm-none-again", new Readable(changing));
