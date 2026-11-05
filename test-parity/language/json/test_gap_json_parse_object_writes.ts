// Property WRITES to `JSON.parse` output (#8098).
//
// A parsed object carries `class_id == 0`. Until #8098 that single fact
// disqualified it from BOTH guarded object-write fast paths — the whole-loop
// numeric clone and the static write PIC — so every `record.field = …` on
// parsed data took the generic `[[Set]]` path for the life of the program
// (measured at 129.5x the instruction count of the identical object-literal
// cell in `benchmarks/object-write-6812/matrix.ts`).
//
// The fix admits a parsed receiver by marking it ORDINARY at birth, which the
// generated guards re-test per object. That means the fast paths now RUN on
// parsed receivers, so every semantic the `class_id != 0` clause used to
// exclude them from has to be checked against node: deleted keys, added keys,
// frozen/sealed/non-extensible receivers, installed descriptors, mutated
// prototypes, dynamic keys, and a parsed object used as a prototype.
//
// Each payload here is a few bytes with an object root, so `js_json_parse`
// takes the eager direct parser, not the >=1 KB top-level-array lazy tape —
// the values these assertions read are fully materialized (#7635).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

function show(v: any): string {
  if (v === undefined) return "undefined";
  if (v === null) return "null";
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}
function line(...parts: any[]): void {
  const out: string[] = [];
  for (let i = 0; i < parts.length; i++) out.push(show(parts[i]));
  console.log(out.join(" "));
}

// (1) The fast-path case itself: a constant-counted nest writing one static
//     key on a uniform prefix of parsed receivers. This is the shape the
//     whole-loop clone matches, and the shape the matrix cell measures.
{
  const objects: any[] = [];
  for (let i = 0; i < 64; i++) objects.push(JSON.parse('{"x":0,"y":1}'));
  for (let r = 0; r < 8; r++) {
    for (let i = 0; i < 64; i++) {
      const object: any = objects[i];
      object.x = r + i;
    }
  }
  let sink = 0;
  for (let i = 0; i < 64; i++) sink += objects[i].x + objects[i].y;
  line("clone", sink, JSON.stringify(objects[0]), JSON.stringify(objects[63]));
}

// (2) Delete an own key, then keep writing the survivors. The delete forks the
//     shape, so a cache trained before it must not keep storing to the old
//     slot.
{
  const o: any = JSON.parse('{"a":1,"b":2,"c":3}');
  o.b = 20;
  delete o.a;
  o.b = 21;
  o.c = 30;
  line("delete", JSON.stringify(o), Object.keys(o).join(","), o.a, "a" in o);
}

// (3) Add a key past the parsed shape, then write both. The add transitions
//     the shape; the pre-transition slot must not be reused for the new key.
{
  const o: any = JSON.parse('{"a":1}');
  o.a = 2;
  o.b = 3;
  o.a = 4;
  o.b = 5;
  line("add", JSON.stringify(o), Object.keys(o).join(","));
}

// (4) freeze / seal / preventExtensions AFTER the site has been trained on a
//     writable receiver of the same shape. A module is strict, so the rejected
//     writes THROW — which is exactly the semantic a fast path that stored
//     unconditionally would lose.
{
  function attempt(fn: () => void): string {
    try {
      fn();
      return "ok";
    } catch (e: any) {
      return e instanceof TypeError ? "TypeError" : "other";
    }
  }
  const warm: any = JSON.parse('{"v":1}');
  warm.v = 2;
  const f: any = JSON.parse('{"v":1}');
  f.v = 2;
  Object.freeze(f);
  const frozenWrite = attempt(() => {
    f.v = 3;
  });
  const se: any = JSON.parse('{"v":1,"w":0}');
  se.v = 2;
  Object.seal(se);
  const sealedWrite = attempt(() => {
    se.v = 3;
  });
  const sealedAdd = attempt(() => {
    se.q = 9;
  });
  const pe: any = JSON.parse('{"v":1}');
  Object.preventExtensions(pe);
  const noextWrite = attempt(() => {
    pe.v = 7;
  });
  const noextAdd = attempt(() => {
    pe.w = 8;
  });
  line("frozen", f.v, Object.isFrozen(f), frozenWrite);
  line("sealed", se.v, se.w, Object.isSealed(se), sealedWrite, sealedAdd);
  line("noextend", pe.v, pe.w, Object.isExtensible(pe), noextWrite, noextAdd);
  line("warm", warm.v);
}

// (5) An accessor descriptor installed over a parsed own data slot must take
//     over the write; a sibling key on the same object must keep working.
{
  const o: any = JSON.parse('{"p":1,"q":2}');
  o.p = 5;
  let seen = -1;
  Object.defineProperty(o, "p", {
    get() {
      return 42;
    },
    set(v: any) {
      seen = v;
    },
    configurable: true,
  });
  o.p = 99;
  o.q = 7;
  line("accessor", o.p, seen, o.q);
}

// (6) A non-writable data descriptor must reject the write (TypeError under
//     the module's strict mode).
{
  const o: any = JSON.parse('{"n":1,"m":2}');
  o.m = 3;
  Object.defineProperty(o, "n", { value: 1, writable: false, configurable: true });
  let threw = "ok";
  try {
    o.n = 2;
  } catch (e: any) {
    threw = e instanceof TypeError ? "TypeError" : "other";
  }
  o.m = 4;
  line("nonwritable", o.n, o.m, threw);
}

// (7) Prototype mutation. An existing OWN data property wins over a setter on
//     the new prototype; a key with no own slot must reach that setter.
{
  let captured = -1;
  const proto = {
    set z(v: number) {
      captured = v;
    },
    get z() {
      return 123;
    },
    set own(v: number) {
      captured = 1000 + v;
    },
  };
  const o: any = JSON.parse('{"own":1}');
  o.own = 2;
  Object.setPrototypeOf(o, proto);
  o.own = 3;
  o.z = 4;
  line("proto", o.own, o.z, captured);
}

// (8) A null-prototype parsed object still takes ordinary writes.
{
  const o: any = JSON.parse('{"k":1}');
  Object.setPrototypeOf(o, null);
  o.k = 2;
  line("nullproto", o.k, Object.getPrototypeOf(o));
}

// (9) Dynamic-key writes on parsed receivers (the 3-way dynamic-key write IC,
//     which carries the same receiver-kind guard).
{
  const objects: any[] = [];
  for (let i = 0; i < 4; i++) objects.push(JSON.parse('{"a":0,"b":0}'));
  const keys = ["a", "b"];
  for (let r = 0; r < 6; r++) {
    for (let i = 0; i < 4; i++) objects[i][keys[r % 2]] = r * 10 + i;
  }
  line("dynkey", JSON.stringify(objects));
}

// (10) A parsed object used as a prototype: writing through the child must
//      create an own property on the child, not overwrite the parent's slot.
{
  const base: any = JSON.parse('{"m":1}');
  const child: any = Object.create(base);
  base.m = 5;
  child.m = 6;
  line("asproto", base.m, child.m, Object.getPrototypeOf(child) === base);
}

// (11) A parsed EMPTY object. Its allocation is the inline-slot floor, so the
//      parse must not initialize more slots than it owns, and growing it by
//      name afterwards must behave.
{
  const o: any = JSON.parse("{}");
  line("empty", JSON.stringify(o), Object.keys(o).length);
  o.a = 1;
  o.b = 2;
  o.c = 3;
  o.a = 4;
  line("empty-grown", JSON.stringify(o));
}

// (12) A polymorphic write site: parsed receivers, object literals and class
//      instances all flowing through one `o.x = …`.
{
  class Cell {
    x = 0;
  }
  const mixed: any[] = [
    JSON.parse('{"x":0}'),
    { x: 0 },
    new Cell(),
    JSON.parse('{"x":0,"extra":1}'),
  ];
  for (let r = 0; r < 4; r++) {
    for (let i = 0; i < mixed.length; i++) mixed[i].x = r * 10 + i;
  }
  const out: string[] = [];
  for (let i = 0; i < mixed.length; i++) out.push(String(mixed[i].x));
  line("poly", out.join(","), JSON.stringify(mixed[3]));
}

// (13) `JSON.parse` creates `__proto__` as a genuine OWN data property, which
//      shadows `Object.prototype`'s accessor — so `o.__proto__ = v` is an
//      ordinary data write, exactly the case the fast path now takes. A fast
//      path that instead reached the accessor would silently reparent the
//      object. `constructor` is the same shape of trap on the read side.
{
  const o: any = JSON.parse('{"__proto__":1,"b":2}');
  line("protokey-own", Object.getOwnPropertyNames(o).join(","), JSON.stringify(o));
  o.__proto__ = 5;
  o.b = 3;
  line("protokey-after", JSON.stringify(o), typeof Object.getPrototypeOf(o), o.b);
  const p: any = JSON.parse('{"constructor":1}');
  p.constructor = 2;
  line("ctorkey", p.constructor, JSON.stringify(p));
}
