// `x === "lit"` with an operand typed as a literal union: identity against
// the other members is only a fast negative. Equal contents from other
// sources (JSON, concatenation, slices, SSO) must still compare equal, and
// unrelated strings unequal.
type Color = "white" | "black";
type Long = "alpha-long-literal-value" | "beta-long-literal-value";
function sign(c: Color): number { return c === "white" ? 1 : -1; }
function isAlpha(x: Long): boolean { return x === "alpha-long-literal-value"; }
class S {
  side: Color = "white";
  tag: Long = "beta-long-literal-value";
  flip(n: number): number {
    let s = 0;
    for (let i = 0; i < n; i++) { s += sign(this.side); this.side = this.side === "white" ? "black" : "white"; }
    return s;
  }
}
const parsed = JSON.parse('["white","black","whit","whitee","alpha-long-literal-value"]') as any[];
const built = ["wh" + "ite", "bla".concat("ck"), "xwhite".slice(1), String.fromCharCode(119, 104, 105, 116, 101)];
const out: any[] = [];
for (const v of [...parsed, ...built, "white", "black", 5, null, undefined, {}]) out.push(sign(v as Color));
console.log(out.join(","));
console.log(isAlpha(parsed[4]), isAlpha("alpha-long" + "-literal-value" as Long), isAlpha("beta-long-literal-value"), isAlpha("x" as Long));
const s = new S();
console.log(s.flip(1001), s.side);
s.side = parsed[1];
console.log(s.flip(3), s.side);
s.side = ("bl" + "ack") as Color;
console.log(s.flip(2), s.side);
// A literal-valued field, appended to, must not change the literal.
class T { label = "ab"; grow(): string { this.label = this.label === "ab" ? "ab" : "ab"; this.label += "c"; return this.label; } }
const t = new T();
console.log(t.grow(), t.grow(), "ab", new T().label);
