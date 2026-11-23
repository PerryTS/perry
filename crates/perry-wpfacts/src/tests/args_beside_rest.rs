//! `arguments` in a function with a rest parameter is the arguments object
//! (every argument), as without one: the HIR appends it after `...r` as a
//! parameter that is no rest itself, and it never takes a positional
//! argument. (Was: it held the argument at its position, `undefined` here,
//! so handing it to unknown code handed nothing and `a.x` stayed proven.)
use super::*;

fn run(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The last read of `x` (`console.log(a.x)`).
fn last_x(pf: &ProgramFacts) -> Verdict {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "x" && !s.is_call)
        .collect();
    s.sort_by_key(|s| s.byte_offset);
    s.last().expect("an x site").verdict.clone()
}

fn program(params: &str) -> String {
    format!(
        "class A {{ x = 1; }}\n\
         const ext: any = (globalThis as any)[process.argv[2]];\n\
         function g({params}) {{ const o: any = arguments; ext(o); }}\n\
         const a = new A();\n\
         g(a);\n\
         console.log(a.x);\n"
    )
}

#[test]
fn arguments_beside_a_rest_parameter_holds_every_argument() {
    for params in ["...r: any[]", "p: any, ...r: any[]", "p: any"] {
        let v = last_x(&run(&program(params)));
        assert!(matches!(v, Verdict::No(_)), "{params}: a.x {v:?}");
    }
}
