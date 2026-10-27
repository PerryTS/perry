declare function gc(): void;
function churn() {
  const keep = [];
  for (let i = 0; i < 100; i++) keep.push({ value: i, text: "move-" + i });
  gc();
  return keep[99].value;
}
// This pending exception has to survive both TLS replacement and relocation.
try {
  try { throw { message: "pending original", children: [{ value: 42 }] }; }
  finally {
    try { throw new Error("inner cleared"); } catch (e: any) { console.log(e.message); }
    console.log("churn", churn());
  }
} catch (e: any) { gc(); console.log(e.message, e.children[0].value); }
// A throw from catch follows a separate finally path with the same obligation.
try {
  try { throw new Error("first"); }
  catch { throw { message: "catch failure", children: [{ value: 73 }] }; }
  finally {
    try { throw new Error("replacement"); } catch {}
    churn();
  }
} catch (e: any) { console.log(e.message, e.children[0].value); }
function abandoned() { const value = { x: 5 }; gc(); throw value; }
function noOwnRoots() { try { abandoned(); } catch { return 23; } }
console.log("rootless catcher", noOwnRoots());
const live = { name: "live catch root", values: [5, 8] };
try { abandoned(); }
catch (e: any) { gc(); console.log(live.name, live.values.join(","), e.x); }
