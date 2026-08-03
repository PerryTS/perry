//! Reader tests, split out of `dialect/mod.rs` for the 2000-line cap.

use super::*;

fn split_corpus(text: &str) -> (String, Vec<String>) {
    let mut skeleton = String::new();
    let mut fns = Vec::new();
    let mut cur: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("define ") {
            cur = Some(String::new());
        }
        match cur.as_mut() {
            Some(f) => {
                f.push_str(line);
                f.push('\n');
                if line == "}" {
                    fns.push(cur.take().unwrap());
                }
            }
            None => {
                skeleton.push_str(line);
                skeleton.push('\n');
            }
        }
    }
    (skeleton, fns)
}

/// Every function in a real perry-emitted corpus file must construct
/// natively and pass the LLVM verifier. This is the reader's primary
/// gate: a form it cannot express fails here, not in a user build.
fn corpus_roundtrip(path: &str) {
    // The corpora are tracked in-tree alongside this reader, so a missing
    // file is a broken checkout, not a branch without artifacts. Skipping
    // would make the reader's primary gate pass vacuously — precisely the
    // failure mode the Linux bring-up had to rule out by hand.
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("corpus file {path} is not readable: {e}"));
    let (skeleton, fns) = split_corpus(&text);
    let ctx = Context::create();
    let module =
        crate::inprocess::parse_ir_text(&ctx, &skeleton, "corpus_skel").expect("skeleton parses");
    for f in &fns {
        predeclare_function_from_text(&ctx, &module, f)
            .unwrap_or_else(|e| panic!("predeclare: {e:#}"));
    }
    let mut n = 0usize;
    for f in &fns {
        n += add_function_from_text(&ctx, &module, f).unwrap_or_else(|e| panic!("{e:#}"));
    }
    assert!(
        n > 1000,
        "expected a real corpus, built only {n} instructions"
    );
    module
        .verify()
        .unwrap_or_else(|e| panic!("verifier rejected native module:\n{}", e.to_string()));
}

#[test]
fn corpus_spike() {
    corpus_roundtrip(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/spike_text.ll"
    ));
}

/// #7302: a try/catch/finally corpus — invoke edges, landing pads,
/// the personality clause on the define, and the inline continuation
/// labels an invoke split leaves behind. Without this the reader's EH
/// support would be exercised only by the async spike, which has one
/// shape (the async rejection boundary) and no nesting.
#[test]
fn corpus_exception_handling() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/eh_text.ll"
    );
    // Assert the subject is LIVE: a corpus that lost its EH forms would
    // still round-trip, and would silently stop testing invoke.
    let text = std::fs::read_to_string(path).expect("eh corpus readable");
    assert!(
        text.matches("invoke ").count() >= 20,
        "eh corpus has no invoke edges left"
    );
    assert!(text.contains("landingpad"), "eh corpus has no landing pad");
    assert!(
        text.contains("personality ptr @perry_eh_personality"),
        "eh corpus lost its personality clause"
    );
    corpus_roundtrip(path);
}

#[test]
fn corpus_batch_kernel() {
    corpus_roundtrip(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../experiments/llvm-inprocess-spike/batch_kernel.ll"
    ));
}
