import DefaultStream, { Stream as NamedStream } from "node:stream";
import BareDefault, { Stream as BareNamed } from "stream";
import { EventEmitter } from "node:events";

// Each import must construct the exported legacy Stream value, with the
// EventEmitter prototype and working inherited methods, rather than a placeholder.
console.log("bindings", DefaultStream === NamedStream, BareDefault === NamedStream, BareNamed === NamedStream);

function check(label: string, stream: any) {
  let total = 0;
  stream.on("tick", (n: number) => { total += n; });
  stream.emit("tick", 2);
  stream.emit("tick", 3);
  console.log(
    label,
    total,
    Object.getPrototypeOf(stream) === NamedStream.prototype,
    stream instanceof DefaultStream,
    stream instanceof NamedStream,
    stream instanceof EventEmitter,
  );
}

check("node default", new DefaultStream());
check("node named", new NamedStream());
check("bare default", new BareDefault());
check("bare named", new BareNamed());
