//! `WeakRef` and `FinalizationRegistry` on the web target.

use super::run_web_source;

// A WeakRef derefs to its live target; a registry registers with and without an
// unregister token, and unregister reports whether the token was registered. A
// non-object target throws the host's TypeError. An async body does the same.
// Expected output is Node's.
#[test]
fn weak_ref_and_finalization_registry_match_node() {
    assert_eq!(
        run_web_source(
            r#"
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
"#
        ),
        "true t true\nundefined undefined\ntrue false true\ntrue\ntrue true undefined\nundefined true false\ntrue\ndone"
    );
}
