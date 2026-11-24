//! SKINROOT35: B1 (a typed array whose buffer is exposed: `.buffer`, a view over a shared buffer) exposes its ELEMENTS only:
//! another view writes bytes, read back as Numbers (BigInts) through an integer index (10.4.5.15-16); no buffer write
//! reaches a named key (10.4.5.2-5). RED on wtq: seed_hard_fields gave the named keys every primitive and any string.
use super::*;
fn reads(buf: &str, src: &str) -> Vec<String> {
    let pf = facts(&format!(
        "class M {{ r = 3; }} const m: any = {{ 'ab-cd': new M() }};
      const ta: any = new Float32Array({buf}, 0, 2); {src}
      const g: any = ta.uuid; console.log(g.r); const t: any = ta.tag; console.log(t.r);"
    ));
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "r" && !s.is_call)
        .map(|s| {
            (
                s.byte_offset,
                format!("{:?}:{:?}:top={}", s.receivers, s.verdict, s.top),
            )
        })
        .collect();
    s.sort();
    s.into_iter().map(|x| x.1).collect()
}
/// r1 + k1: a buffer unknown code holds (⊤) or a `.buffer` read changes no named key (ta.uuid stays undefined: no receiver); the program store `ta.tag = new M()` is still read.
#[test]
fn r1_exposed_buffer_leaves_named_keys() {
    let k = "ta.tag = new M();";
    let a = reads("new ArrayBuffer(8)", k);
    let b = reads(
        "Math.random() < 2 ? new ArrayBuffer(8) : external()",
        "ta.tag = new M(); console.log(ta.buffer.byteLength, ta[0] + 1);",
    );
    eprintln!("SKINROOT35 base={a:?}\nSKINROOT35 b1={b:?}");
    let want = vec![
        "[]:No(\"primitive receiver\"):top=false".to_string(),
        "[\"t.ts:M\"]:Yes".to_string(),
    ];
    for got in [&a, &b] {
        assert!(
            !got[0].contains("t.ts:M") && !got[1].contains("primitive beside"),
            "{got:?} want {want:?}"
        );
    }
}
