function run(re: RegExp, s: string): string[] {
  const out: string[] = [];
  s.replace(re, (m: string) => { out.push(m); return m; });
  return out;
}
const a = run(/[a-z]+|(?=\.)/g, "ab.cd");
if (JSON.stringify(a) !== '["ab","cd"]') throw new Error("A: " + JSON.stringify(a));
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
