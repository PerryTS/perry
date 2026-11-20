// #10430 / #10431: Node's `stream` module value — `require('stream')` and the
// default import `import Stream from "node:stream"` — IS the legacy `Stream`
// constructor, and that constructor extends EventEmitter
// (`lib/internal/streams/legacy.js`: `ObjectSetPrototypeOf(Stream.prototype,
// EE.prototype)` + `ObjectSetPrototypeOf(Stream, EE)`). The module's exports
// (`Readable`, `pipeline`, `promises`, …) hang off it as statics.
//
// Perry used to hand back a separate namespace object: `typeof` said
// "function", but `x instanceof Stream` threw "Right-hand side of 'instanceof'
// is not callable" (node-fetch's `body instanceof Stream`), `Stream !==
// NamedStream`, and nothing inherited from EventEmitter —
// `require('stream').EventEmitter` was undefined, so redis's
// `class ClientSideCacheProvider extends stream_1.EventEmitter` threw
// "Class extends value is not a constructor" at module init.
import { createRequire } from "node:module";
import StreamDefault, {
  Duplex,
  PassThrough,
  Readable,
  Stream,
  Stream as AliasedStream,
  Transform,
  Writable,
  pipeline,
} from "node:stream";
import BareStreamDefault from "stream";
import * as streamNs from "node:stream";
import EventsDefault, { EventEmitter } from "node:events";

const req = createRequire(import.meta.url);
const cjsStream: any = req("stream");
const cjsEvents: any = req("events");
const S: any = StreamDefault;

const t = (name: string, f: () => unknown) => {
  try {
    console.log(name, f());
  } catch (e) {
    console.log(name, "THREW", (e as Error).message);
  }
};

// ── 1. the module value is the named `Stream` constructor (#10431) ──
t("typeof default import:", () => typeof StreamDefault);
t("typeof require('stream'):", () => typeof cjsStream);
t("typeof namespace:", () => typeof streamNs);
t("default === Stream:", () => StreamDefault === Stream);
t("bare default === Stream:", () => BareStreamDefault === Stream);
t("aliased === Stream:", () => AliasedStream === Stream);
t("require('stream') === Stream:", () => cjsStream === Stream);
t("require('stream') === default:", () => cjsStream === StreamDefault);
t("ns.default === Stream:", () => (streamNs as any).default === Stream);
t("ns.Stream === Stream:", () => streamNs.Stream === Stream);
t("require('stream').Stream === require('stream'):", () => cjsStream.Stream === cjsStream);
t("default.Stream === default:", () => S.Stream === S);
t("typeof Stream.prototype:", () => typeof S.prototype);
t("Stream.prototype.constructor === Stream:", () => S.prototype.constructor === Stream);

// ── 2. instanceof with the module value on the right-hand side ──
const readable = new Readable({ read() {} });
const writable = new Writable({
  write(_chunk, _enc, cb) {
    cb();
  },
});
const duplex = new Duplex({
  read() {},
  write(_chunk, _enc, cb) {
    cb();
  },
});
const transform = new Transform({
  transform(chunk, _enc, cb) {
    cb(null, chunk);
  },
});
const passThrough = new PassThrough();
const instances = [
  ["Readable", readable],
  ["Writable", writable],
  ["Duplex", duplex],
  ["Transform", transform],
  ["PassThrough", passThrough],
] as const;
for (const [name, value] of instances) {
  t(`${name} instanceof default:`, () => value instanceof StreamDefault);
  t(`${name} instanceof require('stream'):`, () => value instanceof cjsStream);
  t(`${name} instanceof Stream:`, () => value instanceof Stream);
  t(`${name} instanceof EventEmitter:`, () => value instanceof EventEmitter);
}
t("null instanceof default:", () => (null as any) instanceof StreamDefault);
t("{} instanceof require('stream'):", () => ({}) instanceof cjsStream);
t("EventEmitter instance instanceof Stream:", () => new EventEmitter() instanceof StreamDefault);
t("inline instanceof require:", () => passThrough instanceof req("stream"));

// ── 3. `new Stream()` is an EventEmitter-backed legacy stream ──
function listen(name: string, emitter: any) {
  let got = 0;
  try {
    emitter.on("tick", (n: number) => {
      got += n;
    });
    emitter.emit("tick", 2);
    emitter.emit("tick", 3);
  } catch (e) {
    console.log(name, "THREW", (e as Error).message);
    return;
  }
  console.log(
    name,
    got,
    emitter instanceof Stream,
    emitter instanceof cjsStream,
    emitter instanceof EventEmitter,
  );
}
listen("new Stream():", new Stream());
listen("new default():", new StreamDefault());
listen("new aliased():", new AliasedStream());
listen("new require('stream')():", new cjsStream());
listen("new (any local)():", new S());
listen("Object.create(Stream.prototype):", Object.create(S.prototype));

// ── 4. EventEmitter inheritance (#10430) ──
t("getPrototypeOf(require('stream')) === require('events'):", () =>
  Object.getPrototypeOf(cjsStream) === cjsEvents,
);
t("getPrototypeOf(default) === EventEmitter:", () => Object.getPrototypeOf(StreamDefault) === EventEmitter);
t("require('stream').EventEmitter === require('events'):", () => cjsStream.EventEmitter === cjsEvents);
t("default.EventEmitter === EventEmitter:", () => S.EventEmitter === EventEmitter);
t("typeof default.EventEmitter:", () => typeof (StreamDefault as any).EventEmitter);
t("hasOwn(require('stream'), 'EventEmitter'):", () => Object.hasOwn(cjsStream, "EventEmitter"));
t("typeof require('stream').defaultMaxListeners:", () => typeof cjsStream.defaultMaxListeners);
t("require('stream').once === require('events').once:", () => cjsStream.once === cjsEvents.once);
t("getPrototypeOf(Stream.prototype) === EventEmitter.prototype:", () =>
  Object.getPrototypeOf(S.prototype) === EventEmitter.prototype,
);
t("Stream.prototype instanceof EventEmitter:", () => S.prototype instanceof EventEmitter);

// ── 5. subclassing the module value ──
for (const [name, make] of [
  [
    "class extends require('stream'):",
    () => {
      class Legacy extends req("stream") {}
      return new Legacy();
    },
  ],
  [
    "class extends default import:",
    () => {
      class Legacy extends StreamDefault {}
      return new Legacy();
    },
  ],
  [
    "class extends named Stream:",
    () => {
      class Legacy extends Stream {}
      return new Legacy();
    },
  ],
  [
    "class extends (any local):",
    () => {
      class Legacy extends S {}
      return new Legacy();
    },
  ],
  [
    "class extends require('stream').EventEmitter:",
    () => {
      // redis `@redis/client/dist/lib/client/cache.js` shape.
      const stream_1 = req("stream");
      class ClientSideCacheProvider extends stream_1.EventEmitter {}
      return new ClientSideCacheProvider();
    },
  ],
] as const) {
  try {
    const emitter: any = make();
    let got = 0;
    emitter.on("tick", (n: number) => {
      got += n;
    });
    emitter.emit("tick", 4);
    console.log(name, got, typeof emitter.once, emitter instanceof EventEmitter);
  } catch (e) {
    console.log(name, "THREW", (e as Error).message);
  }
}
{
  class Legacy extends req("stream") {}
  const legacy = new Legacy();
  t("subclass instanceof require('stream'):", () => legacy instanceof cjsStream);
  t("subclass instanceof default:", () => legacy instanceof StreamDefault);
}

// ── 6. module exports reached through the constructor ──
for (const key of [
  "Readable",
  "Writable",
  "Duplex",
  "Transform",
  "PassThrough",
  "pipeline",
  "finished",
  "compose",
  "addAbortSignal",
  "isReadable",
  "getDefaultHighWaterMark",
  "_isUint8Array",
] as const) {
  t(`require('stream').${key} === ns.${key}:`, () => cjsStream[key] === (streamNs as any)[key]);
  t(`default.${key} === ns.${key}:`, () => S[key] === (streamNs as any)[key]);
}
t("default.Readable === Readable (static member):", () => StreamDefault.Readable === Readable);
t("default.pipeline === pipeline (static member):", () => StreamDefault.pipeline === pipeline);
t("typeof require('stream').promises:", () => typeof cjsStream.promises);
t("typeof require('stream').promises.pipeline:", () => typeof cjsStream.promises.pipeline);
t("typeof default.promises.finished:", () => typeof S.promises.finished);
t("keys include module exports:", () =>
  ["Readable", "pipeline", "promises", "Stream", "_isArrayBufferView"].every((k) =>
    Object.keys(cjsStream).includes(k),
  ),
);

// ── 7. the module value drives real streams ──
const collected: string[] = [];
await new Promise<void>((resolve) => {
  cjsStream.pipeline(
    cjsStream.Readable.from(["a", "b", "c"]),
    new cjsStream.Transform({
      transform(chunk: any, _enc: string, cb: (err: Error | null, data?: string) => void) {
        cb(null, String(chunk).toUpperCase());
      },
    }),
    new S.Writable({
      write(chunk: any, _enc: string, cb: () => void) {
        collected.push(String(chunk));
        cb();
      },
    }),
    (err: Error | null | undefined) => {
      console.log("callback pipeline:", err ?? null, collected.join(""));
      resolve();
    },
  );
});
const promised: string[] = [];
await cjsStream.promises.pipeline(
  S.Readable.from(["x", "y"]),
  new cjsStream.Writable({
    write(chunk: any, _enc: string, cb: () => void) {
      promised.push(String(chunk));
      cb();
    },
  }),
);
console.log("promises pipeline:", promised.join(""));
console.log("default.Readable.from:", (await StreamDefault.Readable.from([1, 2, 3]).toArray()).join(","));

// ── 8. controls: `events` already behaved ──
t("events default === EventEmitter:", () => EventsDefault === EventEmitter);
t("require('events') === EventEmitter:", () => cjsEvents === EventEmitter);
t("require('events').EventEmitter === require('events'):", () => cjsEvents.EventEmitter === cjsEvents);
t("typeof require('events').defaultMaxListeners:", () => typeof cjsEvents.defaultMaxListeners);
