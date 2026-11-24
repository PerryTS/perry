// A WeakRef derefs to its live target; a FinalizationRegistry registers with and
// without an unregister token, and unregister reports whether the token was
// registered; a non-object target throws a TypeError.
const target = { name: "t" };
const w = new WeakRef(target);
console.log(w.deref() === target, w.deref()!.name, w instanceof WeakRef);
const r = new FinalizationRegistry((held: string) => { console.log("fin", held); });
const tok = {};
console.log(r.register(target, "h", tok), r.register({}, "x"));
console.log(r.unregister(tok), r.unregister(tok), r instanceof FinalizationRegistry);
try { new WeakRef(1 as any); } catch (e) { console.log(e instanceof TypeError); }
async function inAsync() {
  const wa = new WeakRef(target);
  const ra = new FinalizationRegistry((held: string) => { console.log("fin", held); });
  console.log(wa.deref() === target, wa instanceof WeakRef, ra.register(target, "h", tok));
  console.log(ra.register({}, "x"), ra.unregister(tok), ra.unregister(tok));
  try { new WeakRef(1 as any); } catch (e) { console.log(e instanceof TypeError); }
}
inAsync().then(() => console.log("done"));
