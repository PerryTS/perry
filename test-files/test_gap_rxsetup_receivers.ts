// Receivers whose own properties or prototype differ from a plain RegExp,
// each on a subject the pattern cannot match: user code must still run
// exactly as the specification says.
const show = (label: string, value: unknown) => console.log(label, JSON.stringify(value));
const log: string[] = [];

// Own exec, Symbol.replace and flags getters.
{
  const r: any = /\+/g;
  "abc".replace(r, "_");
  r.exec = function (s: string) { log.push("own exec " + s); return null; };
  show("own exec", ["abc".replace(r, "_"), log.splice(0)]);
  const s: any = /\+/g;
  s[Symbol.replace] = function (str: string) { log.push("own replace " + str); return "own"; };
  show("own symbol replace", ["abc".replace(s, "_"), log.splice(0)]);
  for (const name of ["flags", "global", "unicode", "sticky"]) {
    const f: any = /\+/g;
    Object.defineProperty(f, name, { get() { log.push("get " + name); return name === "flags" ? "g" : true; } });
    show("own getter " + name, ["abc".replace(f, "_"), log.splice(0)]);
  }
  const extra: any = /\+/g;
  extra.note = 1;
  extra.lastIndex = 9;
  show("own expando", ["abc".replace(extra, "_"), extra.lastIndex]);
}

// Subclasses, with and without an exec override.
{
  class Plain extends RegExp {}
  const p = new Plain("\\+", "g");
  p.lastIndex = 4;
  show("subclass", ["abc".replace(p, "_"), p.lastIndex, p.test("abc")]);
  class Logged extends RegExp {
    exec(s: string) { log.push("subclass exec " + s); return super.exec(s); }
  }
  const l = new Logged("\\+", "g");
  show("subclass exec", ["abc".replace(l, "_"), "abc".split(l), log.splice(0)]);
  class Flags extends RegExp {
    get flags() { log.push("subclass flags"); return "g"; }
  }
  show("subclass flags", ["abc".replace(new Flags("\\+"), "_"), log.splice(0)]);
}

// A relinked prototype.
{
  const r: any = /\+/g;
  Object.setPrototypeOf(r, { [Symbol.replace](s: string) { return "relinked " + s; } });
  show("relinked", "abc".replace(r, "_"));
  const back: any = /\+/g;
  Object.setPrototypeOf(back, RegExp.prototype);
  show("relinked back", ["abc".replace(back, "_"), back.lastIndex]);
}

// Object.prototype writes do not change RegExp behaviour.
{
  (Object.prototype as any).exec = function () { return ["object exec"]; };
  show("object prototype", ["abc".replace(/\+/g, "_"), /\+/.test("abc")]);
  delete (Object.prototype as any).exec;
}
