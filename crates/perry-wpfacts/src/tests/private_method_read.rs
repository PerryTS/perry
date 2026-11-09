//! A guarded private method read (`this.#m`, `C.#s`) is the declaring
//! brand's member, as its call is (`Gen::private_method_callee`): the runtime
//! takes it from the class registry (`private_member_get_by_name`), so a
//! later `.call` runs its body (SOUND10 A, node-suite
//! async_method_function_prototype: the read loaded the dispatch key off the
//! receiver's chain, which holds no private method, so the body was claimed
//! inactive while Node ran it).
use super::*;

fn hit_runs(src: &str) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    assert!(pf.body_reach.complete, "activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

#[test]
fn a_read_private_method_runs_through_call() {
    let missed = [
        (
            "instance",
            "class K { #p() { return hit(); } pam() { return this.#p; } } const k = new K(); console.log(k.pam().call(k));",
        ),
        (
            "async instance",
            "class K { async #p() { await 0; return hit(); } pam() { return this.#p; } } const k = new K(); k.pam().call(k).then((v: any) => console.log(v));",
        ),
        (
            "static",
            "class K { static #s() { return hit(); } static pam() { return K.#s; } } console.log(K.pam().call(K));",
        ),
    ]
    .into_iter()
    .filter(|(_, src)| !hit_runs(&format!("function hit() {{ return 1; }}\n{src}")))
    .map(|(what, _)| what)
    .collect::<Vec<_>>();
    assert!(missed.is_empty(), "{missed:?}");
}

/// Control: a private method read but never called runs no body.
#[test]
fn a_read_private_method_not_called_does_not_run() {
    assert!(!hit_runs(
        "function hit() { return 1; }\nclass K { #p() { return hit(); } pam() { return this.#p; } } console.log(typeof new K().pam());"
    ));
}
