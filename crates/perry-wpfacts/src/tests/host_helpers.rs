//! Host helpers whose operands are validated or ToString'd: `path.parse` and
//! `crypto.randomBytes` ToPrimitive their argument; a tagged template reads
//! its cooked strings and calls nothing on them.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        import * as path from "node:path";
        import * as crypto from "node:crypto";
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        class ValueOf {{ size = 3; valueOf(): any {{ (globalThis as any).hostTake(this); return 1; }} toString(): any {{ (globalThis as any).hostTake(this); return "a"; }} }}
        function g(a: any) {{ return a.size; }}
        {body}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

#[test]
fn path_parse_runs_to_string() {
    let v = verdict("const o: any = new ValueOf(); try { path.parse(o); } catch (_e) {} console.log(g(o));");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn random_bytes_of_a_plain_object_is_proven() {
    let v = verdict("const o: any = new Svc(); try { crypto.randomBytes(o); } catch (_e) {} console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}
