// A program that replaces String.prototype.charCodeAt: the result of
// s.charCodeAt(i) is whatever the replacement returns, so an array key built
// from it must not be treated as a code unit. Every form of the char-code
// key is covered: a direct call, a block-scoped let, a hoisted var written
// twice (the qs encode shape).
const t: any[] = ["a", "b", "c"];
(t as any)["1.5"] = "<1.5 prop>";
(t as any)["NaN"] = "<NaN prop>";
function direct(input: any) { const s = String(input); return t[s.charCodeAt(0)]; }
function blockLet(s: string) { for (let i = 0; i < 1; i++) { let c = s.charCodeAt(i); return t[c]; } return null; }
function hoisted(input: any) {
  const s = String(input);
  const out: any[] = [];
  for (var i = 0; i < s.length; ++i) {
    var c = s.charCodeAt(i);
    if (c > 0xffff) c = 0x10000 + ((c & 0x3ff) << 10);
    out.push(t[c]);
  }
  return out;
}
console.log(JSON.stringify([direct("x"), blockLet("x"), hoisted("xy")]));
const original = String.prototype.charCodeAt;
// DEC 250 (intrinsic-only): a primitive string's methods come only from
// String.prototype. The replace is refused now (expected file); the reads
// after it still run.
try { (String.prototype as any).charCodeAt = function (this: string, i: number) { return i === 0 ? 1.5 : original.call(this, i) % 3; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log(JSON.stringify([direct("x"), blockLet("x"), hoisted("xy"), "x".charCodeAt(0)]));
try { (String.prototype as any).charCodeAt = function () { return NaN; }; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log(JSON.stringify([direct("x"), blockLet("x"), hoisted("xy")]));
try { (String.prototype as any).charCodeAt = original; } catch (e) { console.log("DEC 250: " + (e as Error).message); }
console.log(JSON.stringify([direct("x"), blockLet("x"), hoisted("xy")]));
