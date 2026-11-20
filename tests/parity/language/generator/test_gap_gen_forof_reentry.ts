// Resuming a generator from inside its own body throws TypeError
// ("already executing") and leaves the generator usable; the outer loop goes on.
let self: Generator<number>;
const out: string[] = [];
function* g(): Generator<number> {
  yield 1;
  try { self.next(); } catch (e) { out.push("next " + (e instanceof TypeError)); }
  try { for (const v of self) out.push("never " + v); } catch (e) { out.push("forof " + (e instanceof TypeError)); }
  try { self.return(5 as any); } catch (e) { out.push("return " + (e instanceof TypeError)); }
  yield 2;
  yield 3;
}
self = g();
for (const v of self) out.push("v" + v);
console.log(out.join("\n"));
console.log(JSON.stringify(self.next()));
