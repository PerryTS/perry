// An object born from a spread of nothing (`{ ...undefined }`) has a keyless
// layout with a null key list. Spreading it AFTER another source took the
// prefixed birth path, which read the source's key list unconditionally and
// segfaulted. marked's parse(src) does exactly this:
// `let r = { ...options }, o = { ...this.defaults, ...r }` with no options.

const empty: any = { ...undefined };
const fromNull: any = { ...null };
console.log(JSON.stringify({ ...{ a: 1 }, ...empty }));
console.log(JSON.stringify({ ...{ a: 1, b: "x" }, ...fromNull, c: 3 }));

function parse(src: string, options?: any) {
  const defaults = { async: false, gfm: true, renderer: null, silent: false };
  const r = { ...options };
  const o = { ...defaults, ...r };
  return src + " " + JSON.stringify(o);
}
console.log(parse("one"));
console.log(parse("two", { gfm: false }));
for (let i = 0; i < 3; i++) console.log(parse("loop" + i, i === 1 ? { silent: true } : undefined));
