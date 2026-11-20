// #9430 — a global scan must follow ECMAScript's `RegExpExec` loop: an empty
// match is kept even when it sits exactly where the previous match ended, and
// the scan then advances one code unit. Rust's `regex` and `fancy-regex`
// iterators use the opposite rule ("ignore an empty match immediately after a
// match"), which silently drops the trailing empty match of `"a".match(/a*/g)`
// — and every interior one too.
//
// Every row below uses only the `g` flag (plus whatever engine the pattern
// selects). No `m`, so #9408's LineTerminator gap cannot reach these.

function show(label: string, value: unknown): void {
  console.log(label + " => " + JSON.stringify(value));
}

// ---- String.prototype.match, global -------------------------------------
show("match a* / a", "a".match(/a*/g));
show("match a* / aXa", "aXa".match(/a*/g));
show("match b* / ab", "ab".match(/b*/g));
show("match x* / abc", "abc".match(/x*/g));
show("match a* / (empty)", "".match(/a*/g));
show("match a* / aa", "aa".match(/a*/g));
show("match a*/ ab a", "ab a".match(/a*/g));
show("match (?:) / abc", "abc".match(/(?:)/g));
show("match a|/ ab", "ab".match(/a|/g));
show("match \\d*/ a1b22", "a1b22".match(/\d*/g));
show("match a+ / aXa", "aXa".match(/a+/g));

// ---- matchAll ------------------------------------------------------------
function allOf(s: string, re: RegExp): string[] {
  const out: string[] = [];
  for (const m of s.matchAll(re)) out.push(JSON.stringify(m[0]) + "@" + m.index);
  return out;
}
show("matchAll a* / a", allOf("a", /a*/g));
show("matchAll b* / ab", allOf("ab", /b*/g));
show("matchAll a* / aXa", allOf("aXa", /a*/g));
show("matchAll x* / abc", allOf("abc", /x*/g));
show("matchAll (a)* / a", allOf("a", /(a)*/g));
show("matchAll (?<=,) / a,b,", allOf("a,b,", /(?<=,)/g));
show("matchAll (?=b) / abb", allOf("abb", /(?=b)/g));

// matchAll honours a non-zero lastIndex on the source regex.
const mall = /a*/g;
mall.lastIndex = 1;
show("matchAll a* / aXa @1", (() => {
  const out: string[] = [];
  for (const m of "aXa".matchAll(mall)) out.push(JSON.stringify(m[0]) + "@" + m.index);
  return out;
})());
const mall2 = /(?<=a)/g;
mall2.lastIndex = 1;
show("matchAll (?<=a) / ab @1", (() => {
  const out: string[] = [];
  for (const m of "ab".matchAll(mall2)) out.push(JSON.stringify(m[0]) + "@" + m.index);
  return out;
})());

// ---- replace with a string replacement ----------------------------------
show("replace a*->{} / a", "a".replace(/a*/g, "<>"));
show("replace a*->{} / aXa", "aXa".replace(/a*/g, "-"));
show("replace b*->{} / ab", "ab".replace(/b*/g, "-"));
show("replace x*->{} / abc", "abc".replace(/x*/g, "-"));
show("replace a*->$& / aXa", "aXa".replace(/a*/g, "[$&]"));
show("replace (?:)->- / ab", "ab".replace(/(?:)/g, "-"));
show("replaceAll a*->- / a", "a".replaceAll(/a*/g, "-"));
show("replace (?<=a)->! / aba", "aba".replace(/(?<=a)/g, "!"));
show("replace (?=b)->! / abb", "abb".replace(/(?=b)/g, "!"));
show("replace (a)*->[$1] / a", "a".replace(/(a)*/g, "[$1]"));
show("replace (?<n>a)*->[$<n>] / a", "a".replace(/(?<n>a)*/g, "[$<n>]"));

// ---- replace with a callback --------------------------------------------
function collect(s: string, re: RegExp): string[] {
  const out: string[] = [];
  s.replace(re, (m: string, ...rest: unknown[]) => {
    out.push(JSON.stringify(m) + "@" + rest[rest.length - 2]);
    return m;
  });
  return out;
}
show("cb a* / a", collect("a", /a*/g));
show("cb a* / aXa", collect("aXa", /a*/g));
show("cb b* / ab", collect("ab", /b*/g));
show("cb (?=b) / abb", collect("abb", /(?=b)/g));
show("cb (?<=,) / a,b,", collect("a,b,", /(?<=,)/g));
show("cb (a)* / aXa", collect("aXa", /(a)*/g));
show("cb [a-z]+|(?=\\.) / ab.cd", collect("ab.cd", /[a-z]+|(?=\.)/g));
show("cbOut a* / a", "a".replace(/a*/g, (m: string) => "[" + m + "]"));
show("cbOut b* / ab", "ab".replace(/b*/g, (m: string) => "[" + m + "]"));

// ---- split: the spec's `e == p` skip must stay unchanged ----------------
show("split a* / a", "a".split(/a*/));
show("split a* / aXa", "aXa".split(/a*/));
show("split b* / ab", "ab".split(/b*/));
show("split (?:) / abc", "abc".split(/(?:)/));
show("split x* / abc", "abc".split(/x*/));
show("split , / a,b,", "a,b,".split(/,/));
// EXCLUDED, a third and unrelated root cause: `split` by a pattern only
// fancy-regex can compile (lookaround / backreferences) does not run the
// spec's `RegExp.prototype [ @@split ]` algorithm at all. That fallback walks
// `find_iter` and slices between the matches, so it emits a trailing `""` the
// spec's `q < size` bound never reaches — `"a,b,".split(/(?<=,)/)` is
// `["a,","b,",""]` here and `["a,","b,"]` in Node — and it splices no captured
// groups: `"aXbXc".split(/((?<=a)X)/)` is `["a","bXc"]` here and
// `["a","X","bXc"]` in Node. The `regex`-engine rows above take
// `spec_regex_split`, which is correct, so this is a lane gap, not a scan gap.
// (The runtime test `fancy_lookbehind_split` currently pins the wrong answer.)
// #9427 widens its reach: rewriting a `/m` anchor into lookaround moves those
// patterns onto the same lane, so `"a\r\nb".split(/^/gm)` takes it too and gains
// a spurious LEADING "" — `["","a\r","\n","b"]` against Node's `["a\r","\n","b"]`.
show("split a*,2 / aXa", "aXa".split(/a*/, 2));
show("split ,* / ab,", "ab,".split(/,*/));
show("split \\d* / a1b", "a1b".split(/\d*/));
show("split (a)* / bab", "bab".split(/(a)*/));

// ---- non-global controls: one match, no scan ---------------------------
show("match a* nog / aXa", "aXa".match(/a*/));
show("replace a* nog / aXa", "aXa".replace(/a*/, "-"));
show("search a* / Xa", "Xa".search(/a*/));
