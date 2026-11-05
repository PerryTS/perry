function run(re: RegExp, s: string): string[] {
  const out: string[] = [];
  s.replace(re, (m: string) => { out.push(m); return m; });
  return out;
}
// #9430: the alternation's zero-width `(?=\.)` branch matches at index 2 —
// exactly where the `[a-z]+` match before it ended — and ECMAScript's scan
// loop KEEPS that match and then advances one code unit. This assertion used
// to read '["ab","cd"]', which is what a Rust match iterator produces (it
// discards an empty match sitting at the previous match's end) and what Perry
// therefore printed; Node has always thrown here. The stored expected-output
// file says `OK`, so the divergence was invisible to the runner.
const a = run(/[a-z]+|(?=\.)/g, "ab.cd");
if (JSON.stringify(a) !== '["ab","","cd"]') throw new Error("A: " + JSON.stringify(a));
const d = run(/(?=\.)/g, "a.b.c");
if (JSON.stringify(d) !== '["",""]') throw new Error("D: " + JSON.stringify(d));

const rePropName = /[^%.[\]]+|\[(?:(-?\d+(?:\.\d+)?)|(["'])((?:(?!\2)[^\\]|\\.)*?)\2)\]|(?=(?:\.|\[\])(?:\.|\[\]|%$))/g;
const result: any[] = [];
"%String.prototype.indexOf%".replace(rePropName, function (m: any, num: any, q: any, sub: any) {
  result[result.length] = q ? sub : (num || m);
  return m;
} as any);
if (JSON.stringify(result) !== '["String","prototype","indexOf"]') throw new Error("rePropName: " + JSON.stringify(result));
console.log("OK");
