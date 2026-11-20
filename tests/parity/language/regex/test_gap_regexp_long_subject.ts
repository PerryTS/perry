// RegExp operations on heap-string subjects, including subjects longer than
// 64 KiB, through one fused method site (`run`: plain-parameter arguments, so
// the lookup and the call are fused and a proven site calls the builtin test
// directly) and through the ordinary exec/replace/match/split paths. Every
// row prints JSON; output must equal Node's.
function run(re: any, s: any): any {
  return re.test(s);
}
function t(name: string, f: () => any): void {
  let line: string;
  try {
    line = name + ": " + JSON.stringify(f());
  } catch (e: any) {
    line = name + ": throw " + (e && e.constructor && e.constructor.name);
  }
  console.log(line);
}

const KB64 = 64 * 1024;
const big = "a".repeat(KB64 + 7) + "needle" + "b".repeat(1000) + "needle" + "c".repeat(33);
// Two-byte UTF-16 units and a surrogate pair past 64 KiB: indices count units.
const wide = "é".repeat(KB64 + 3) + "\u{1F600}needle" + "è".repeat(10);
const lines: string[] = [];
for (let i = 0; i < 8; i++) lines.push("line " + i + " of an ordinary heap-allocated subject");

t("heap_test_fused", () => {
  const re = /ordinary heap/;
  const out: boolean[] = [];
  for (let i = 0; i < 64; i++) run(re, lines[i % 8]);
  for (const s of lines) out.push(run(re, s));
  out.push(run(re, "no match in this heap string at all"));
  return [out, re.lastIndex];
});
t("long_test_fused", () => {
  const re = /needle/;
  const a: boolean[] = [];
  for (let i = 0; i < 64; i++) a.push(run(re, big));
  return [a.every((x) => x), run(re, "x".repeat(KB64 + 1)), re.lastIndex];
});
t("long_test_global_progress", () => {
  const re = /needle/g;
  const seen: number[] = [];
  for (let i = 0; i < 4; i++) {
    const m = run(re, big);
    seen.push(m ? 1 : 0, re.lastIndex);
  }
  return seen;
});
t("long_test_sticky", () => {
  const re = /needle/y;
  re.lastIndex = KB64 + 7;
  const a = run(re, big);
  const at = re.lastIndex;
  const b = run(re, big);
  return [a, at, b, re.lastIndex];
});
t("long_test_lastindex_past_end", () => {
  const re = /needle/g;
  re.lastIndex = big.length + 5;
  return [run(re, big), re.lastIndex];
});
t("long_test_lastindex_valueof", () => {
  const re: any = /needle/;
  let calls = 0;
  re.lastIndex = { valueOf() { calls++; return 3; } };
  const r = [run(re, big), run(re, big)];
  return [r, calls, typeof re.lastIndex];
});
t("long_exec_index", () => {
  const re = /n(ee)(dle)/g;
  const m1 = re.exec(big)!;
  const m2 = re.exec(big)!;
  const m3 = re.exec(big);
  return [m1.index, m1[1], m1[2], m2.index, re.lastIndex, m3];
});
t("long_exec_wide", () => {
  const re = /needle/u;
  const m = re.exec(wide)!;
  return [m.index, wide.length, /\u{1F600}/u.exec(wide)!.index];
});
t("long_replace", () => {
  const r1 = big.replace(/needle/, "N");
  const r2 = big.replace(/needle/g, (m: string, off: number) => "<" + off + ">");
  const r3 = wide.replace(/needle/g, "$&!");
  return [r1.length, r1.indexOf("N"), r2.length, r2.slice(KB64 + 5, KB64 + 20), r3.length, r3.slice(-15)];
});
t("long_match_split_search", () => {
  const all = big.match(/needle/g)!;
  const it = [...big.matchAll(/ne(e)dle/g)].map((m) => m.index);
  const parts = big.split(/needle/);
  return [all.length, it, parts.length, parts.map((p) => p.length), big.search(/needle/), wide.search(/needle/)];
});
t("long_no_match", () => {
  const s = "q".repeat(KB64 * 2);
  return [/needle/.test(s), /q+$/.test(s), s.replace(/x/g, "y").length, /(q{3})$/.exec(s)!.index];
});
