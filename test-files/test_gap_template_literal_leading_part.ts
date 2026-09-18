// A template literal that OPENS on a substitution (`` `${x}...` ``, the
// overwhelmingly common shape: no literal text before the first `${`) used
// to unconditionally seed its desugared concat chain with a real, always-
// empty `Expr::String("")` part — the leading quasi was the only one that
// never got the "skip when empty" guard interior/trailing quasis already
// had. That wasted classification slot survived all the way to
// `js_string_concat_chain`, and for a single-substitution template
// (`` `${x}` ``) it also defeated the >=3-part minimum for the n-way
// concat-chain fold entirely, forcing the pairwise path to concatenate a
// literal "" for nothing.
//
// Separately, a `number`-typed parameter's template substitution used to
// keep its `StringCoerce` wrapper (materializing an intermediate heap
// string via `js_string_coerce` before the chain call ever saw it) because
// only a *proven* (not merely declared) non-pointer local elided it — and a
// plain public-body function parameter is never proof-bearing, only
// declaration-bearing. Both fixes are covered together here because they
// compound: the leading substitution is exactly where the elided part
// becomes the flattened chain's FIRST entry rather than an interior one.

function tpl(s: string, n: number): string {
  return `${s}:${n}`;
}

// The leading-substitution shapes the elision above targets.
function lead1(n: number): string {
  return `${n}`;
}
function lead2(n: number, s: string): string {
  return `${n}:${s}`;
}
function leadInt(i: number): string {
  return `${i}!`;
}

// Multi-part chain (>=3 substitutions), still opening on `${`.
function multi(a: number, b: string, c: number, d: string): string {
  return `${a}-${b}-${c}-${d}`;
}

// Integer vs non-integer interpolation.
console.log(tpl("abc", 3), tpl("abc", 3.5), tpl("abc", -0), tpl("abc", NaN));
console.log(leadInt(0), leadInt(-1), leadInt(1000000));

// Leading substitution, single and multi-part.
console.log(lead1(42), lead1(1 / 3), lead1(-0), lead1(Infinity));
console.log(lead2(7, "x"), lead2(2.5, ""), lead2(-9, "tail"));

// Empty string operands on both sides of the elided number.
console.log(tpl("", 5), tpl("", 5.25), lead2(0, ""));

// Multi-part chain.
console.log(multi(1, "a", 2, "b"), multi(-1.5, "", 0, "z"));

// Result short enough for SSO (<=5 bytes total) alongside a longer one.
console.log(`${1}${2}`, `${"ab"}${12}`, `${lead1(9)}${"x"}${9}`);

// Non-ASCII and surrogate-pair content flowing through the leading part and
// through an interior number part.
const emoji = String.fromCharCode(0xd83d) + String.fromCharCode(0xde00);
console.log(`${emoji}:${3}`, `${"héllo"}:${7.5}`, `${3}:${emoji}`);
// Adjacent lone surrogates split across TWO parts must still canonicalize
// into one astral scalar in the chained result.
const hi = String.fromCharCode(0xd83d);
const lo = String.fromCharCode(0xde00);
console.log(`${hi}${lo}:${1}`, (`${hi}${lo}:${1}`).length);

// A lying `number` annotation reaching the LEADING (now-unwrapped) position:
// the elision must fall back to the exact same coercion `String(x)` uses,
// not misread the bits.
console.log(lead1("nine" as any), lead1(true as any), lead1(null as any));
console.log(lead1({ toString: () => "OBJ" } as any));
const both = {
  valueOf() {
    return 111;
  },
  toString() {
    return "STR";
  },
};
console.log(lead1(both as any), lead2(both as any, "s"));

// A substitution whose (elided) coercion still must not double-evaluate or
// reorder relative to its neighbors.
let calls = 0;
function counted(): any {
  calls++;
  return 7;
}
console.log(`${counted()}:${counted()}`, calls);

// Hot loop shape: repeated leading-substitution template, integer and
// fractional, accumulated.
let acc = "";
for (let i = 0; i < 50; i++) acc = `${i}:${i / 3}`;
console.log(acc, acc.length);
