// Gap test for #9408. ECMAScript §22.2.2.6 makes `^` and `$` under the `m`
// flag match at EVERY LineTerminator — LF, CR, U+2028 LINE SEPARATOR and
// U+2029 PARAGRAPH SEPARATOR — which is exactly the set #9218 taught a
// non-dotAll `.` to exclude. Rust's `(?m)` recognises LF only, so Perry's
// translation used to see one line where Node sees three.
//
// `\r\n` is the case that most naturally comes out wrong: the pair is TWO
// line terminators, so a `/^.*$/gm` scan must produce an EMPTY match between
// the CR and the LF.
//
// This file is byte-compared with `node --experimental-strip-types` by the gap
// suite. The `u`/non-`m` rows are controls: the translation must not move the
// anchors for a pattern that never asked for multiline.

const LS = "\u2028";
const PS = "\u2029";

function show(label: string, value: unknown): void {
  console.log(label + ":" + JSON.stringify(value));
}

function endsWithTerminator(s: string): boolean {
  return s.length > 0 && "\n\r\u2028\u2029".includes(s[s.length - 1]);
}

const subjects: Array<[string, string]> = [
  ["lf", "one\ntwo"],
  ["cr", "one\rtwo"],
  ["crlf", "one\r\ntwo"],
  ["lfcr", "one\n\rtwo"],
  ["ls", "one" + LS + "two"],
  ["ps", "one" + PS + "two"],
  ["mixed", "a\rb\nc" + LS + "d" + PS + "e"],
  ["leading", "\rone"],
  ["trailing", "one\r"],
  ["only", "\r"],
  ["empty", ""],
  ["none", "onetwo"],
];

// The headline scan: every line of the subject, anchors on both ends.
for (const [name, subject] of subjects) {
  show("lines-" + name, subject.match(/^.*$/gm));
  show("lines-u-" + name, subject.match(/^.*$/gmu));
  // dotAll makes `.` eat the terminator, so the whole subject is one match.
  // Subjects that END with a terminator are skipped here: a global scan must
  // then report one more EMPTY match at the end of the input, and dropping it
  // is a SEPARATE, pre-existing defect with nothing to do with the anchors
  // (`"a".match(/a*/g)` is `["a"]` in Perry and `["a",""]` in Node).
  if (!endsWithTerminator(subject)) show("lines-s-" + name, subject.match(/^.*$/gms));
  // Without `m` the anchors stay whole-input anchors.
  show("lines-nom-" + name, subject.match(/^.*$/g));
}

// Anchors as zero-width insertion points. `^` before each line, `$` after
// each line; `\r\n` must take TWO insertions, not one.
for (const [name, subject] of subjects) {
  show("insert-caret-" + name, subject.replace(/^/gm, ">"));
  show("insert-dollar-" + name, subject.replace(/$/gm, "<"));
  show("insert-both-" + name, subject.replace(/^|$/gm, "|"));
  show("insert-caret-nom-" + name, subject.replace(/^/g, ">"));
  show("insert-dollar-nom-" + name, subject.replace(/$/g, "<"));
}

// Non-empty anchored patterns: the shapes the cc bundle actually ships.
show("gitdir", "line\r\ngitdir: /w/t\r\nline".match(/^gitdir:\s*(.+)$/m));
show("diffgit", /^diff --git /m.test("index x\r\ndiff --git a b\r\n"));
show("osrelease", 'NAME="x"\r\nID="alpine"\r\n'.match(/^ID=["']?(\S+?)["']?\s*$/m));
show("heading", "text\r\n## Title\r\nmore".match(/^## /gm));
show("atx", "x\r\n### Deep heading\r\ny".match(/^#+\s+(.+)$/m));
show("trailing-ws", "a  \r\nb\t\r\nc  ".replace(/[ \t]+$/gm, ""));
show("bullet", "- a\r\n- b\r\n- c".match(/^- (.+)$/gm));

// `$` immediately before each terminator, `^` immediately after.
for (const [name, subject] of subjects) {
  show("dollar-idx-" + name, [...subject.matchAll(/$/gm)].map((m) => m.index));
  show("caret-idx-" + name, [...subject.matchAll(/^/gm)].map((m) => m.index));
}

// `re.exec` with a non-zero `lastIndex` is NOT covered here. Perry hands the
// engine `&subject[lastIndex..]`, so every assertion loses its left context:
// `\A` (and `\b`, and a lookbehind) is evaluated against the slice rather
// than the string. That is an independent defect — `/^b/g` with
// `lastIndex = 1` already matched "ab" before this fixture existed, with no
// `m` flag anywhere — and it needs the positional search APIs
// (`captures_at` / `captures_from_pos`), not a translation change.

// `^` and `$` inside a character class are ordinary literals and must not be
// rewritten. `\^` / `\$` are escaped literals for the same reason.
show("class-literal", "a^b$c".match(/[$^]/g));
show("class-literal-m", "a^b$c".match(/[$^]/gm));
show("escaped-caret", "a^b".match(/\^/gm));
show("escaped-dollar", "a$b".match(/\$/gm));
show("negated-class-m", "x\ry".match(/[^\r]/gm));

// Anchors nested inside groups and alternations still see the flag.
show("group-anchor", "a\rb".match(/(?:^b)/m));
show("alt-anchor", "a\rb".match(/(^b|^a)/gm));
show("lookahead-anchor", "a\rb".match(/(?=^b)b/m));
show("anchored-quantifier", "aa\rbb".match(/^(\w)\1$/gm));

// Case-insensitive and dotAll combinations exercise the flag prefix.
show("ci-anchor", "A\rB".match(/^b$/gim));
show("dotall-anchor", "a\rb".match(/^a$/gms));
show("all-flags", "A\rB".match(/^./gimsu));

// #9263 / #9216 controls: the word-boundary markers and the empty-class
// spellings share this translator and must survive alongside the anchors.
show("word-boundary-m", "a\rb".match(/^\b\w+\b$/gm));
show("nonword-boundary-m", "Ω\rΩ".match(/^\B/gm));
show("any-class-m", "a\rb".match(/^[^]$/gm));
show("empty-class-m", "a\rb".match(/^[]$/gm));
show("word-complement-m", "a\rb".match(/^[\w\W]$/gm));
