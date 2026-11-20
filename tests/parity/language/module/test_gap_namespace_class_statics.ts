import * as buffer from "node:buffer";
import * as url from "node:url";
import * as crypto from "node:crypto";
import * as stream from "node:stream";
import * as events from "node:events";
import * as cluster from "node:cluster";

function attempt(f: () => any): string {
  try { return String(f()); } catch (e: any) { return "threw " + e.constructor.name; }
}

// Keep #11941's constructor-value route working for every Buffer static.
const a = buffer.Buffer.from("a");
const b = buffer.Buffer.from("b");
console.log(buffer.Buffer.compare(a, b), buffer.Buffer.concat([a, b]).toString());
const compare = buffer.Buffer.compare;
console.log(compare(b, a), buffer.Buffer.isEncoding("utf8"), buffer.Buffer.alloc(2).length);

// These direct calls used to invent an undispatched native class call.
console.log(url.URL.canParse("https://example.com/a"), url.URL.canParse("invalid"));
const canParse = url.URL.canParse;
console.log(canParse("https://example.com"), canParse("/a", "https://example.com"));
console.log(url.URL["canParse"]("https://example.com"));
console.log(url["URL"].canParse(...["https://example.com"]));
console.log(attempt(() => crypto.KeyObject.from(null)));
const keyFrom = crypto.KeyObject.from;
console.log(attempt(() => keyFrom(null)));

const emitter = new events.EventEmitter();
console.log(stream.Stream.getMaxListeners(emitter));
const streamMax = stream.Stream.getMaxListeners;
console.log(streamMax(emitter), stream.Stream.defaultMaxListeners);

// Module-wide native entries also serve inherited statics; preserve main.
console.log(events.EventEmitterAsyncResource.getMaxListeners(emitter));
console.log(cluster.Worker.getMaxListeners(emitter));
