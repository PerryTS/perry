// #9429 — `exec`/`test` with `lastIndex > 0` must evaluate the pattern against
// the FULL subject, not against `subject.slice(lastIndex)`. Slicing destroys
// the context every zero-width assertion depends on, and it is wrong in both
// directions: `^`/`\b`/`(?<!…)` start matching where they must not, and
// `(?<=…)` stops matching where it must.
//
// WHICH LineTerminators a `/m` anchor recognises is a separate root cause
// (#9408, fixed in #9427) with its own fixture, so almost every `/m` row here
// uses `\n` alone and this fixture pins one cause. The two `\r\n` / U+2028
// sweeps at the bottom are the exception, kept because they are the case
// #9429 was reported from and they need BOTH fixes: with #9408 alone the loop
// never terminates, with this fix alone it stops at [0, 5].

function ex(re: RegExp, s: string, li: number): string {
  re.lastIndex = li;
  const m = re.exec(s);
  const body = m === null ? "null" : JSON.stringify(m[0]) + "@" + m.index;
  return body + " li=" + re.lastIndex;
}

function tst(re: RegExp, s: string, li: number): string {
  re.lastIndex = li;
  const hit = re.test(s);
  return (hit ? "T" : "F") + " li=" + re.lastIndex;
}

function show(label: string, value: string): void {
  console.log(label + " => " + value);
}

// ---- `^` (no `m`): only position 0 of the SUBJECT ------------------------
show("^b g@0  ab", ex(/^b/g, "ab", 0));
show("^b g@1  ab", ex(/^b/g, "ab", 1));
show("^b g@2  ab", ex(/^b/g, "ab", 2));
show("^b y@1  ab", ex(/^b/y, "ab", 1));
show("^a g@0  ab", ex(/^a/g, "ab", 0));
show("^a g@1  ab", ex(/^a/g, "ab", 1));

// ---- `^` under /m: after a LineTerminator in the SUBJECT ----------------
show("^b gm@0 a\\nb", ex(/^b/gm, "a\nb", 0));
show("^b gm@1 a\\nb", ex(/^b/gm, "a\nb", 1));
show("^b gm@2 a\\nb", ex(/^b/gm, "a\nb", 2));
show("^n gm@1 a\\nb", ex(/^n/gm, "a\nb", 1));

// ---- `$` -----------------------------------------------------------------
show("b$ g@0  ab", ex(/b$/g, "ab", 0));
show("b$ g@1  ab", ex(/b$/g, "ab", 1));
show("$  g@2  ab", ex(/$/g, "ab", 2));
show("a$ gm@0 a\\nb", ex(/a$/gm, "a\nb", 0));
show("a$ gm@1 a\\nb", ex(/a$/gm, "a\nb", 1));

// ---- `\b` / `\B` ---------------------------------------------------------
show("\\bb g@1 ab", ex(/\bb/g, "ab", 1));
show("\\bb g@1 a b", ex(/\bb/g, "a b", 1));
show("\\bb g@2 a b", ex(/\bb/g, "a b", 2));
show("\\Bb g@1 ab", ex(/\Bb/g, "ab", 1));
show("\\Bb g@1 a b", ex(/\Bb/g, "a b", 1));
show("\\bb y@1 ab", ex(/\bb/y, "ab", 1));
show("\\bb y@2 a b", ex(/\bb/y, "a b", 2));

// ---- lookbehind (fancy-regex lane) --------------------------------------
show("(?<=a)b g@0 ab", ex(/(?<=a)b/g, "ab", 0));
show("(?<=a)b g@1 ab", ex(/(?<=a)b/g, "ab", 1));
show("(?<=a)b g@2 ab", ex(/(?<=a)b/g, "ab", 2));
show("(?<=a)b y@1 ab", ex(/(?<=a)b/y, "ab", 1));
show("(?<!a)b g@1 ab", ex(/(?<!a)b/g, "ab", 1));
show("(?<!a)b g@1 xb", ex(/(?<!a)b/g, "xb", 1));
show("(?<=b)   g@2 ab", ex(/(?<=b)/g, "ab", 2));
show("(?<=ab)c g@2 abc", ex(/(?<=ab)c/g, "abc", 2));
show("(?<=ab)c y@2 abc", ex(/(?<=ab)c/y, "abc", 2));

// ---- lookahead (fancy-regex lane) ---------------------------------------
show("a(?=b) g@0 abab", ex(/a(?=b)/g, "abab", 0));
show("a(?=b) g@1 abab", ex(/a(?=b)/g, "abab", 1));
show("a(?=b) g@3 abab", ex(/a(?=b)/g, "abab", 3));
show("(?=b)  g@0 ab", ex(/(?=b)/g, "ab", 0));
show("(?=b)  g@1 ab", ex(/(?=b)/g, "ab", 1));
show("(?!b)  y@1 ab", ex(/(?!b)/y, "ab", 1));

// ---- lastIndex past the end resets and reports no match ------------------
show("a* g@5 ab", ex(/a*/g, "ab", 5));
show("a* g@2 ab", ex(/a*/g, "ab", 2));
show("a* y@5 ab", ex(/a*/y, "ab", 5));
show("b  g@9 ab", ex(/b/g, "ab", 9));

// ---- the same, through `test` -------------------------------------------
show("test ^b g@1 ab", tst(/^b/g, "ab", 1));
show("test (?<=a)b g@1 ab", tst(/(?<=a)b/g, "ab", 1));
show("test \\bb g@1 ab", tst(/\bb/g, "ab", 1));
show("test ^b y@1 ab", tst(/^b/y, "ab", 1));
show("test a* g@5 ab", tst(/a*/g, "ab", 5));
// A non-global, non-sticky regex ignores lastIndex entirely.
const plain = /^b/;
plain.lastIndex = 1;
show("test ^b plain ab", (plain.test("ab") ? "T" : "F") + " li=" + plain.lastIndex);

// ---- a hand-driven `^`/gm exec loop must terminate -----------------------
function anchorSweep(re: RegExp, s: string): string {
  const at: number[] = [];
  let m: RegExpExecArray | null;
  let guard = 0;
  while ((m = re.exec(s)) !== null) {
    at.push(m.index);
    if (m[0] === "") re.lastIndex = re.lastIndex + 1;
    if (++guard > 24) { at.push(-1); break; }
  }
  return JSON.stringify(at);
}
show("sweep ^/gm one\\ntwo", anchorSweep(/^/gm, "one\ntwo"));
show("sweep ^/gm a\\nb\\nc", anchorSweep(/^/gm, "a\nb\nc"));
show("sweep $/gm one\\ntwo", anchorSweep(/$/gm, "one\ntwo"));
show("sweep \\b/g  ab cd", anchorSweep(/\b/g, "ab cd"));
show("sweep (?<=,)/g a,b,c", anchorSweep(/(?<=,)/g, "a,b,c"));
// Needs #9408 (in #9427) as well as this fix — see the header note.
show("sweep ^/gm one\\r\\ntwo", anchorSweep(/^/gm, "one\r\ntwo"));
show("sweep ^/gm a\\u2028b", anchorSweep(/^/gm, "a\u2028b"));

// ---- a plain global exec loop still walks the whole subject --------------
function execSweep(re: RegExp, s: string): string {
  const out: string[] = [];
  let m: RegExpExecArray | null;
  let guard = 0;
  while ((m = re.exec(s)) !== null) {
    out.push(m[0] + "@" + m.index);
    if (m[0] === "") re.lastIndex = re.lastIndex + 1;
    if (++guard > 24) { out.push("!"); break; }
  }
  return JSON.stringify(out);
}
show("sweep \\w+/g  ab cd ef", execSweep(/\w+/g, "ab cd ef"));
show("sweep ^\\w/gm  ab\\ncd", execSweep(/^\w/gm, "ab\ncd"));
show("sweep (?<=a)./g  ab ac", execSweep(/(?<=a)./g, "ab ac"));
show("sweep .(?=b)/g   ab cb", execSweep(/.(?=b)/g, "ab cb"));
