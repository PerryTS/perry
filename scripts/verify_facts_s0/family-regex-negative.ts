// family_reach, a second family: a RegExp is born. Under
// PERRY_FACTS_SABOTAGE=famreach:regex_engine the fact says the regex engine
// is never created or observed; the witness at `js_regexp_new` must report
// the birth ("family").
function build(source: string): RegExp { return new RegExp(source + "(b+)c"); }
const r = build("a");
console.log(r.test("abbc"), r.source);
