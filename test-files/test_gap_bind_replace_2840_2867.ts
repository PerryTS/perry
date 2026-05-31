// Gap test for #2840 (Function.prototype.bind) and #2867 (String.replace callback args).

// --- #2840: Function.prototype.bind ---

function f(this: any, a: number, b: number) {
  return [this.x, a, b];
}
const g = f.bind({ x: 1 }, 2);
console.log("bind this+partial:", JSON.stringify(g(3))); // [1,2,3]
console.log("bind distinct:", g !== f); // true
console.log("bind length:", g.length); // 1
console.log("bind name:", g.name); // bound f

// bind with no partial args
function h(this: any, a: number) {
  return this.y + a;
}
const hb = h.bind({ y: 10 });
console.log("bind no-partial:", hb(5)); // 15
console.log("bind no-partial length:", hb.length); // 1

// bind with multiple partial args
function add3(a: number, b: number, c: number) {
  return a + b + c;
}
const a3 = add3.bind(null, 1, 2);
console.log("bind multi-partial:", a3(3)); // 6
console.log("bind multi-partial length:", a3.length); // 1

// --- #2867: String.replace regex callback arguments ---

function argsFor(label: string, pattern: RegExp, input: string) {
  let seen: string[] = [];
  input.replace(pattern, function (...args: any[]) {
    seen = args.map((v) =>
      typeof v === "object" && v !== null ? JSON.stringify(v) : String(v),
    );
    return "x";
  });
  console.log(label, seen.length, seen.join("|"));
}

argsFor("no-capture", /b/, "abc"); // no-capture 3 b|1|abc
argsFor("captures", /(a)(b)(c)/, "abc"); // captures 6 abc|a|b|c|0|abc
argsFor("named", /(?<word>\w+)-(?<num>\d+)/, "abc-123"); // named 6 abc-123|abc|123|0|abc-123|{"word":"abc","num":"123"}

// Global flag still calls the callback per match with the full arg list.
const out = "abcabc".replace(/(a)(b)/g, (m, p1, p2, off, str) => {
  return `[${m}:${p1}:${p2}:${off}:${str}]`;
});
console.log("replace global:", out);
