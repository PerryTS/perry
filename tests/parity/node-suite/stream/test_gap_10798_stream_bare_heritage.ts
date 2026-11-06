// #10798: `class X extends Stream` (the bare legacy `node:stream` base, not
// one of its subclasses) threw `TypeError: ... is not a constructor`. This
// blocked `nodemailer`, which loads `class XOAuth2 extends Stream` on
// import.
//
// PR #10649 fixed the analogous dynamic-heritage-dispatch gap in
// `js_fetch_or_value_super` (crates/perry-runtime/src/object/global_this/
// fetch_globals.rs) for `Readable`/`Writable`/`Duplex`/`Transform`, but its
// match list stopped short of `Stream` — the base those four themselves
// derive from.
//
// Unlike #10745 (`PassThrough`), this is NOT the deeper HIR-level gap:
// `canonical_native_parent_name` (crates/perry-hir/src/lower_decl/
// class_decl.rs) never recognized ANY spelling of `Stream` as a native
// parent — unlike Readable/Writable/Duplex/Transform, which have a fast
// static path for a plain `import`, EVERY `extends Stream` heritage shape
// (bare ident, namespace member, CJS destructured `require`) already
// reaches the same dynamic `js_fetch_or_value_super` dispatch #10649
// patches, uniformly. And `Stream` carries no hidden per-instance state to
// pre-seed in the first place — in Node it is literally `EventEmitter` plus
// a `pipe()` prototype method (`lib/internal/streams/legacy.js`), so the
// fix reuses the existing `js_event_emitter_subclass_init` shim rather than
// adding a stream-specific one.
import { Stream as ImportedStream } from "node:stream";
import * as streamNs from "node:stream";
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
const cjsStreamModule: any = req("stream");
const { Stream: RequiredStream } = cjsStreamModule;

class BareTap extends ImportedStream {}
class NamespaceTap extends streamNs.Stream {}
class CjsTap extends RequiredStream {}

function run(name: string, T: any) {
  let t: any;
  try {
    t = new T();
  } catch (e) {
    console.log(name, "THREW (construct)", (e as Error).message);
    return;
  }
  let got = 0;
  const seen: string[] = [];
  t.on("data", (c: any) => {
    got++;
    seen.push(String(c));
  });
  t.emit("data", "a");
  t.emit("data", "b");
  console.log(
    name,
    "count:", got,
    "values:", seen.join(","),
    "typeof pipe:", typeof t.pipe,
    "typeof on:", typeof t.on,
    "typeof once:", typeof t.once,
    "instanceof Stream:", t instanceof ImportedStream,
  );
}

run("bare extends Stream            ", BareTap);
run("namespace extends stream.Stream", NamespaceTap);
run("cjs destructured extends Stream", CjsTap);
