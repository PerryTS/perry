//! A protocol touch of a token a native model bore (`Model::implicit` on a
//! box `Object.prototype.valueOf` made, 7.1.18 ToObject) shares one root
//! per code position for the native runs it starts (`chain_call`,
//! `Model::touch_of_solver_born`): with a root per touch, named after the
//! root's own fresh call, each touch bore a new box, touched again, without
//! end (r10av4: a stack overflow after 43,608 nested replays).
use super::*;

/// The facts of `src` with a work bound, on a thread of its own: `None`
/// when the solve has not returned in `secs`.
fn settled(src: &str, secs: u64) -> Option<ProgramFacts> {
    let src = src.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(64 << 20)
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

/// `Object.prototype[k] = arr[k]` with an unknown `k`: every key of
/// `Object.prototype` may hold any member of `Array.prototype` or of
/// `Object.prototype`, so an element's `toLocaleString` may be `valueOf`
/// (a box of the element), whose ToPrimitive may run
/// `Array.prototype.toLocaleString` on it again. Main ad7a583ab9: a stack
/// overflow (it aborts the test binary: run this filter on its own).
#[test]
fn touch_of_a_native_born_box_settles() {
    for prog in [
        "const k: string = process.argv[2] ?? \"x\";
         const arr: any = [1, 2];
         const P: any = Object.prototype;
         P[k] = arr[k];
         const a: any[] = [1, \"s\", true];
         console.log(a.toLocaleString(), \"\" + ({} as any));",
        "const k: string = process.argv[2] ?? \"x\";
         const arr: any = [1, 2];
         const P: any = Object.prototype;
         P[k] = arr[k];
         const o: any = { v: 1 };
         const a: any[] = [1, \"s\", o];
         console.log(`${a}`, a.toLocaleString(), String(o));",
    ] {
        let pf = settled(prog, 300).unwrap_or_else(|| panic!("no fixed point in 300 s: {prog}"));
        assert!(
            !pf.stats.aborted,
            "work bound hit: {prog}\n{}",
            pf.dump_text()
        );
    }
}

/// Named control: the touch of a program-born object keeps its own root,
/// so what its `toString` returns stays exactly itself.
#[test]
fn touch_of_a_program_object_keeps_its_result() {
    let src = r#"
const o: any = { toLocaleString() { return "abc"; } };
const a: any[] = [o];
const s = a.toLocaleString();
console.log(s.length); // SITE
"#;
    let pf = settled(src, 120).expect("settles");
    assert!(!pf.stats.aborted);
    let at = src.find("// SITE").unwrap();
    let line = src[..at].rfind('\n').map_or(0, |p| p + 1) as u32;
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| !s.is_call && s.key == "length" && (line..at as u32).contains(&s.byte_offset))
        .collect();
    assert!(!sites.is_empty(), "missing SITE\n{}", pf.dump_text());
    assert!(
        sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        "{}",
        pf.dump_text()
    );
}
