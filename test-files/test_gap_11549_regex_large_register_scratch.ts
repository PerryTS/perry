// #11549: a pattern with more than 32 match registers borrows the thread's
// lent scratch instead of building and freeing owned buffers per call, and
// regex scratch no longer counts as released external GC pressure. Results
// must be unchanged, including for nested searches (a replacer callback that
// runs another large pattern while the outer search holds the lent cell) and
// across programs of different sizes on one thread.

// dotenv's line pattern: 42 registers.
const LINE = /(?:^|^)\s*(?:export\s+)?([\w.-]+)(?:\s*=\s*?|:\s+?)(\s*'(?:\\'|[^'])*'|\s*"(?:\\"|[^"])*"|\s*`(?:\\`|[^`])*`|[^#\r\n]+)?\s*(?:#.*)?(?:$|$)/mg;

let doc = "# generated\nexport NODE_ENV=production\nPORT=8080\n";
doc += 'MULTI="line one\nline two"\n';
doc += "SINGLE='single # not a comment'\n";
doc += "EMPTY=\n  SPACED  =  spaced value  \n";
for (let k = 0; k < 6; k++) doc += "KEY_" + k + "=value_" + k + " # trailing " + k + "\n";

function parse(src: string): Record<string, string> {
  const out: Record<string, string> = {};
  LINE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = LINE.exec(src)) != null) out[m[1]] = (m[2] || "").trim();
  return out;
}

// Many iterations with churn in between, so collections run mid-loop.
let sig = 0;
let last = "";
for (let i = 0; i < 3000; i++) {
  const parsed = parse(doc);
  const junk: string[] = [];
  for (let j = 0; j < 20; j++) junk.push("x" + i + "_" + j);
  sig = (sig + Object.keys(parsed).length + junk.length) % 1000003;
  last = JSON.stringify(parsed);
}
console.log(sig, last);

// A wide alternation: more registers than the old 32-slot cell.
const WIDE = new RegExp(Array.from({ length: 40 }, (_, i) => "(w" + i + ")").join("|"));
console.log(["w0", "w17", "w39", "w40"].map((s) => (WIDE.exec(s) || []).filter((x) => x).join("/")));

// Nested: the outer search holds the lent cell while the callback searches.
const nested = "a=1\nb=2\nc=3\n".replace(/^(\w)=(\d)$/gm, (_all, k, v) => {
  const inner = parse(k.toUpperCase() + "_X=" + v + " # c\n");
  return k + "->" + JSON.stringify(inner);
});
console.log(nested);

// Alternate small and large programs on the same thread.
const SMALL = /(\d+)-(\d+)/;
const out: string[] = [];
for (let i = 0; i < 5; i++) {
  out.push((SMALL.exec("x" + i + "-" + (i * 3) + "y") || []).slice(1).join(":"));
  out.push(String(Object.keys(parse("K" + i + "=v" + i + "\n")).length));
}
console.log(out.join(","));
