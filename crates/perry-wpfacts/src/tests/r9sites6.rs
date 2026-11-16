//! R9SITES6 fixtures: RULE RP in the activation pass.
//!
//! The activation pass (DECISIONS 225) walks one body for every call of a
//! function and marks every function `no_summary`; RULE RP still holds
//! there: a function whose every completion returns its unreassigned plain
//! parameter returns, at each call, the argument that call passes
//! (ECMA-262 10.2.1.3, 10.2.11). Two rest-parameter loggers that pass
//! their own arrays through one such helper each `shift()` their own front
//! (a constant without `%`: Node util.format consumes no later argument),
//! so `toJSON` of a later argument never runs and its body stays inactive.
use super::*;

/// Whether function `name`'s body can run (the activation pass's reach).
fn runs(src: &str, name: &str) -> bool {
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
        .find(|f| f.name == name)
        .expect(name)
        .id;
    pf.body_reach.bodies.iter().any(|(_, i, _)| *i == id)
}

/// The three.js shape: the helper reads, branches, stores into its
/// parameter and returns it; `first` is the warn logger's first argument.
fn program(first: &str, enh: &str) -> String {
    format!(
        "function hit( o: any ) {{ return {{ k: o.q }}; }}
class A {{ q: any; constructor() {{ this.q = Number( process.argv.length ); }} toJSON() {{ return hit( this ); }} }}
const a = new A();
{enh}
function warn( ...params: any[] ) {{ params = enh( params ); const m = 'T.' + params.shift(); console.warn( m, ...params ); }}
function error( ...params: any[] ) {{ params = enh( params ); const m = 'T.' + params.shift(); console.error( m, ...params ); }}
warn( {first}, a ); error( 'y', [ a ], a ); warn( 'z' ); error( 'w' );
"
    )
}

const ENH: &str = "function enh( params: any ) {
  const message = params[ 0 ];
  if ( typeof message === 'string' && message.startsWith( 'TSL:' ) ) {
    const stackTrace = params[ 1 ];
    if ( stackTrace && stackTrace.isStackTrace ) { params[ 0 ] += ' ' + stackTrace.getLocation(); }
    else { params[ 1 ] = 'Stack trace not available.'; }
  }
  return params;
}";

// S1 RED: each logger gets its own array back in the activation pass too:
// both fronts are constants without `%`, toJSON (and `hit`) never run.
#[test]
fn r9sites6_s1_activation_ret_param_keeps_rest_arrays() {
    let on = runs(&program("'x'", ENH), "hit");
    eprintln!("R9SITES6 S1 hit runs={on}");
    assert!(!on, "S1 toJSON body active");
}

// S2 KEEP: the warn logger's first argument is unknown (`process.argv[2]`):
// it may hold `%j`, util.format serializes `a`: toJSON runs.
#[test]
fn r9sites6_s2_keep_unknown_format() {
    let on = runs(&program("String( process.argv[ 2 ] )", ENH), "hit");
    eprintln!("R9SITES6 S2 hit runs={on}");
    assert!(on, "S2 lost the reached toJSON");
}

// S3 KEEP: a helper that reassigns its parameter is no RP function: its
// return is shared, each shift may read the other array's object.
#[test]
fn r9sites6_s3_keep_reassigned_helper() {
    let on = runs(
        &program(
            "'x'",
            "function enh( params: any ) { if ( ! params ) params = [ a ]; return params; }",
        ),
        "hit",
    );
    eprintln!("R9SITES6 S3 hit runs={on}");
    assert!(on, "S3 lost the shared return");
}
