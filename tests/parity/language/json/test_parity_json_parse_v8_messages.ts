// JSON.parse SyntaxError messages follow V8's wording.
const cases = ["", "{", "{\"a\":}", "[1,]", "tru", "{\"a\" 1}", "\"abc", "01", "[1 2]", "{\"a\":1,}", "nul", "1e", "\"\\x\"", "{a:1}", "[", "undefined", "NaN"];
for (const c of cases) {
  try { JSON.parse(c); console.log("ok", c); } catch (e: any) { console.log(e.name + ": " + e.message); }
}
const r = (JSON as any).rawJSON("123");
console.log(Object.isFrozen(r), r.rawJSON);
for (const bad of ["{}", " 1"]) {
  try { (JSON as any).rawJSON(bad); console.log("no throw"); } catch (e: any) { console.log(e.name); }
}
