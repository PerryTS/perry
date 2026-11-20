//! A builtin method's Invoke of a member of its receiver
//! (`Array.prototype.toString`'s `this.join()`, `toLocaleString`'s of each
//! element) is one call per (receiver, member) (`collections::recv_call`),
//! not one per call that ran the method: when the member is the method
//! itself the solve still settles, and a member that is not keeps its result.
use super::*;

/// The facts of `src` with a work bound, on a thread of its own with a
/// large stack: `None` when the solve has not returned in `secs` (a model
/// that never settles fails here instead of taking the harness down).
fn settled(src: &str, secs: u64) -> Option<ProgramFacts> {
    let src = src.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let pf = facts_with(
                &src,
                FactsConfig {
                    max_work: Some(5_000_000),
                    ..Default::default()
                },
            );
            let _ = tx.send(pf);
        })
        .expect("spawns");
    rx.recv_timeout(std::time::Duration::from_secs(secs)).ok()
}

/// Is every read of `key` on the line marked `// SITE` proven?
fn site_proven(pf: &ProgramFacts, src: &str, key: &str) -> Result<bool, String> {
    let at = src.find("// SITE").expect("marked site");
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == key && (line..at as u32).contains(&s.byte_offset))
        .collect();
    if sites.is_empty() {
        return Err(format!("missing SITE .{key}\n{}", pf.dump_text()));
    }
    Ok(sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))))
}

/// `o.join = Array.prototype.toString` (the runtime throws a RangeError):
/// each run of `toString` on `o` invokes `o.join`, which is `toString` on
/// `o` again. A call per run never settles (main 07dafbd9ff: a stack
/// overflow); one call per (`o`, `join`) does. The same for `"" + p` (its
/// ToPrimitive finds `toString`) and an array holding itself under
/// `toLocaleString`.
#[test]
fn receiver_member_that_is_the_method_itself_settles() {
    for prog in [
        "const o: any = {}; o.join = Array.prototype.toString;
         try { console.log(Array.prototype.toString.call(o)); } catch (e) { console.log(\"RangeError\"); }",
        "const p: any = { toString: Array.prototype.toString, join: Array.prototype.toString };
         try { console.log(\"\" + p); } catch (e) { console.log(\"RangeError\"); }",
        "const a: any[] = [1]; a.push(a); const b: any[] = [a]; a.push(b);
         console.log(a.toLocaleString(), b.toLocaleString());",
    ] {
        let pf = settled(prog, 120).unwrap_or_else(|| panic!("no fixed point in 120 s: {prog}"));
        assert!(!pf.stats.aborted, "work bound hit: {prog}\n{}", pf.dump_text());
    }
}

/// Named control: a `join` that is not the method keeps its exact result.
/// What `toString` returns is what `join` returns: the payload, read as
/// exactly itself, and `"x"`.
#[test]
fn receiver_member_that_is_not_the_method_keeps_its_result() {
    let src = r#"
class Payload { n = 1; }
const payload = new Payload();
const o: any = {}; o.join = function () { return payload; };
const r: any = Array.prototype.toString.call(o);
console.log(r.n); // SITE
"#;
    let pf = settled(src, 120).expect("settles");
    assert!(!pf.stats.aborted);
    assert_eq!(site_proven(&pf, src, "n"), Ok(true));
    let src = r#"
const o: any = {}; o.join = function () { return "x"; };
const r: any = Array.prototype.toString.call(o);
console.log(r.length); // SITE
"#;
    let pf = settled(src, 120).expect("settles");
    assert!(!pf.stats.aborted);
    assert_eq!(site_proven(&pf, src, "length"), Ok(true));
}
