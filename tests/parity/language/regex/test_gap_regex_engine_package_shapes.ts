// Regex shapes the package workloads spend their time in (uuid validate,
// jws JWS_REGEX, dotenv LINE, validator isByteLength split, node-cron), plus
// the edge cases the engine's fast paths for them must keep exact: folded
// classes (including the two non-ASCII characters that fold into ASCII under
// `u`), counted repeats, lazy repeats before a literal, captures, lastIndex,
// sticky and global iteration. Output must match Node byte-for-byte.

function show(label: string, value: unknown): void {
  console.log(label + " " + JSON.stringify(value));
}

// uuid validate: counted folded classes in an alternation.
const UUID =
  /^(?:[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}|00000000-0000-0000-0000-000000000000|ffffffff-ffff-ffff-ffff-ffffffffffff)$/i;
for (const u of [
  "3b241101-e2bb-4255-8caf-4136c566a962",
  "9C5B94B1-35AD-49BB-B118-8E8FC24ABF80",
  "00000000-0000-0000-0000-000000000000",
  "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF",
  "3b241101-e2bb-4255-8caf-4136c566a96",
  "3b241101-e2bb-4255-8caf-4136c566a9621",
  "3b241101-e2bb-9255-8caf-4136c566a962",
  "3b241101-e2bb-4255-7caf-4136c566a962",
  "3b24110g-e2bb-4255-8caf-4136c566a962",
  "",
]) {
  show("uuid " + u, UUID.test(u));
}

// jws: lazy repeats before a literal, then an optional greedy capture.
const JWS = /^[a-zA-Z0-9\-_]+?\.[a-zA-Z0-9\-_]+?\.([a-zA-Z0-9\-_]+)?$/;
for (const t of [
  "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl",
  "a.b.c",
  "a.b.",
  "a..c",
  ".b.c",
  "a.b.c.d",
  "a.b",
  "abc.def.gh!i",
  "abc.def.ghi\n",
]) {
  show("jws " + JSON.stringify(t), JWS.test(t));
  show("jws-exec " + JSON.stringify(t), JWS.exec(t));
}
show("lazy-cap", /^([a-z]+?)([a-z]{2})$/.exec("abcdef"));
show("lazy-min", /^(a{2,}?)(a*)b$/.exec("aaaaab"));
show("lazy-bound", /^(x{1,3}?)\.$/.exec("xxx."));
show("lazy-bound-over", /^(x{1,3}?)\.$/.exec("xxxx."));
show("lazy-fold-lit", /^([a-z]+?)K/i.exec("abckdef"));
show("lazy-fold-lit-u", /^([a-z]+?)k/iu.exec("abKdef"));
show("lazy-class-cont", /^(\w+?)(\d+)$/.exec("abc123"));
show("lazy-any", /<(.+?)>/g[Symbol.match]("<a><bb><ccc>"));
show("lazy-nonascii", /^(.+?)é(.*)$/.exec("abcé123é"));
show("lazy-astral", /^(.+?)\u{1F600}/u.exec("ab\u{1F600}c"));

// Folded classes: legacy and unicode case equivalence.
show("fold-kelvin-legacy", /[a-z]/i.test("K"));
show("fold-kelvin-u", /[a-z]/iu.test("K"));
show("fold-long-s-u", /^[s]$/iu.test("ſ"));
show("fold-long-s-legacy", /^[s]$/i.test("ſ"));
show("fold-kelvin-class-u", /^[K]+$/iu.test("kKK"));
show("fold-negated", /^[^a-f]+$/i.test("GHIJ"));
show("fold-negated-miss", /^[^a-f]+$/i.test("GHIa"));
show("fold-negated-u", /[^k]/iu.test("K"));
show("fold-digits", /^[0-9a-f]{4}$/i.test("aF09"));
show("fold-sigma", /^[σ]+$/i.test("Σσς"));
show("fold-sigma-u", /^[σ]+$/iu.test("Σσς"));
show("fold-greek-range", "ΑΒΓαβγ".match(/[α-γ]+/gi));
show("fold-latin1", /^[à-þ]+$/i.test("ÀÉÎõü"));
show("fold-word", "a_Z9ſK".match(/\w/giu));
show("fold-word-legacy", "a_Z9ſK".match(/\w/gi));
show("fold-dash", /^[a-z-]+$/i.test("Ab-C"));
show("fold-v", /^[\p{Lu}--[A-C]]+$/iv.test("DEF"));

// Counted repeats: exact, bounded, captured, sticky, global.
show("count-exact", /^\d{3}-\d{4}$/.test("555-1234"));
show("count-short", /^\d{3}-\d{4}$/.test("55-1234"));
show("count-range", "12 1234 123456".match(/\b\d{2,4}\b/g));
show("count-cap", /^(\d{1,2})-(\d{1,2})$/.exec("7-12"));
show("count-sticky", (() => { const r = /\d{2}/y; r.lastIndex = 1; const m = r.exec("a12b"); return [m, r.lastIndex]; })());
show("count-sticky-miss", (() => { const r = /\d{2}/y; r.lastIndex = 2; const m = r.exec("a12b"); return [m, r.lastIndex]; })());
show("count-global", (() => { const r = /[a-c]{2}/g; const out: unknown[] = []; let m; while ((m = r.exec("abcabcab")) !== null) out.push([m[0], m.index, r.lastIndex]); return out; })());
show("cron-l", [/^l-\d{1,2}$/i.test("L-12"), /^l-\d{1,2}$/i.test("l-123"), /^[0-7]l$/i.test("5L")]);
show("count-nonascii", /^(é{2})(.)$/.exec("ééx"));
show("count-astral-u", /^(.{2})$/u.exec("\u{1F600}a"));

// dotenv LINE over a small document.
const LINE =
  /(?:^|^)\s*(?:export\s+)?([\w.-]+)(?:\s*=\s*?|:\s+?)(\s*'(?:\\'|[^'])*'|\s*"(?:\\"|[^"])*"|\s*`(?:\\`|[^`])*`|[^#\r\n]+)?\s*(?:#.*)?(?:$|$)/mg;
const doc = "# c\nexport A=1\nB = two words # note\nC='q # x'\nD=\"m\\nn\"\nE:  colon\n  F  =  spaced  \nG=\n";
const pairs: unknown[] = [];
let m: RegExpExecArray | null;
while ((m = LINE.exec(doc)) != null) pairs.push([m[1], m[2], m.index, LINE.lastIndex]);
show("dotenv", pairs);

// validator isByteLength: split on a two-way alternation.
for (const s of ["hello", "caf%C3%A9", "", "%20%20", "a%2"]) {
  show("bytelen " + s, s.split(/%..|./));
}
show("split-limit", "a,b;c".split(/[,;]/, 2));
show("split-cap", "a1b22c".split(/(\d+)/));

// Whitespace class (more than eight ranges) as a repeated atom.
show("ws", "a \t  b".split(/\s+/));
show("ws-trim", "  x y  ".replace(/^\s+|\s+$/g, ""));
show("ws-nonascii", /^\s{3}$/.test(" 　﻿"));

// String-template replacements, whose output is built from spans of the
// subject and the template: ASCII and non-ASCII on either side, every
// substitution form, empty pieces and empty results.
show("rep-g", "a-b-c".replace(/-/g, "+"));
show("rep-1", "Hello".replace(/l/, "L"));
show("rep-empty", "---".replace(/-/g, ""));
show("rep-all-empty", "".replace(/x*/g, "y"));
show("rep-dollar", "a-b".replace(/(-)/g, "[$1|$&|$`|$'|$$]"));
show("rep-named", "2026-09-27".replace(/(?<y>\d+)-(?<m>\d+)-(?<d>\d+)/, "$<d>/$<m>/$<y>"));
show("rep-missing-group", "ab".replace(/(a)(x)?/, "[$2]"));
show("rep-nonascii-subject", "café-thé".replace(/-/g, " & "));
show("rep-nonascii-template", "a-b".replace(/-/g, "→"));
show("rep-astral", "a\u{1F600}b".replace(/\u{1F600}/u, "$&$&"));
show("rep-lone", "a\ud800b".replace(/b/, "$`"));
show("rep-escape", 'say "hi"\\n'.replace(/\\n/g, "\n").replace(/^"|"$/g, ""));
show("rep-dotenv-quote", "'quoted value'".replace(/^(['"`])([\s\S]*)\1$/gm, "$2"));
show("rep-sticky", (() => { const r = /a/y; r.lastIndex = 1; return ["baa".replace(r, "X"), r.lastIndex]; })());
show("rep-replaceAll", "x.y.z".replaceAll(/\./g, "$&$&"));
show("rep-long", ("ab-".repeat(300)).replace(/-/g, "+").length);
