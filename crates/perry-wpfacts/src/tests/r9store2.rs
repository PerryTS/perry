//! R9STORE2 fixtures: the console formatter and `toJSON`.
//!
//! Node lib/internal/util/inspect.js `formatWithOptions`: only a String
//! FIRST argument is a format string; `%j` in it serializes the argument
//! it consumes (JSON.stringify: 25.5.2.2 SerializeJSONProperty calls
//! `toJSON`); every other argument is shown by `inspect`, which never
//! calls `toJSON`.
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

const CLS: &str = "function hit( o: any ) { return { k: o.q }; }
class A { q: any; constructor() { this.q = Number( process.argv.length ); } toJSON() { return hit( this ); } }
const a = new A();
";

/// The three.js `log` shape: a rest logger, the front shifted off and
/// prefixed (a constant without `%`), the rest spread after it.
fn logger(call: &str) -> String {
    format!(
        "{CLS}function log( ...params: any[] ) {{ const message = 'T.' + params.shift(); console.log( message, ...params ); }}
{call}
"
    )
}

// C1 RED: `log( 'x', a )`: the format string is 'T.x' (no `%`): `a` is
// shown by inspect, `toJSON` never runs.
#[test]
fn r9store2_c1_rest_logger_const_front() {
    let on = runs(&logger("log( 'x', a ); log( 'y' );"), "hit");
    eprintln!("R9STORE2 C1 hit runs={on}");
    assert!(!on, "C1 toJSON body active");
}

// C2 KEEP: `console.log( '%j', a )`: `%j` serializes `a`: reached.
#[test]
fn r9store2_c2_pct_j_reaches() {
    let on = runs(&format!("{CLS}console.log( '%j', a );\n"), "hit");
    assert!(on, "C2 %j toJSON not reached");
}

// C3 KEEP: an unknown first argument may hold `%j`: reached.
#[test]
fn r9store2_c3_unknown_first_reaches() {
    let on = runs(
        &format!("{CLS}console.log( String( process.argv[2] ), a );\n"),
        "hit",
    );
    assert!(on, "C3 unknown format string: toJSON not reached");
}

// C4 KEEP: `console.log( a )`: inspect only: not reached.
#[test]
fn r9store2_c4_lone_object_not_reached() {
    let on = runs(&format!("{CLS}console.log( a );\n"), "hit");
    assert!(!on, "C4 lone argument reached toJSON");
}

/// three.core.js lines 1908..2064 verbatim (`_setConsoleFunction`, `log`,
/// `enhanceLogMessage`, `warn`, `error`).
const THREE_LOG: &str = "let _setConsoleFunction = null;

/**
 * Sets a custom function to handle console output.
 *
 * This allows external code to intercept and handle console.log, console.warn,
 * and console.error calls made by three.js, which is useful for custom logging,
 * testing, or debugging workflows.
 *
 * @param {Function} fn - The function to handle console output. Should accept
 *                        (type, message, ...params) where type is 'log', 'warn', or 'error'.
 */
function setConsoleFunction( fn ) {

	_setConsoleFunction = fn;

}

/**
 * Gets the currently set custom console function.
 *
 * @return {Function|null} The custom console function, or null if not set.
 */
function getConsoleFunction() {

	return _setConsoleFunction;

}

/**
 * Logs an informational message with the 'THREE.' prefix.
 *
 * If a custom console function is set via setConsoleFunction(), it will be used
 * instead of the native console.log. The first parameter is treated as the
 * method name and is automatically prefixed with 'THREE.'.
 *
 * @param {...any} params - The message components. The first param is used as
 *                          the method name and prefixed with 'THREE.'.
 */
function log( ...params ) {

	const message = 'THREE.' + params.shift();

	if ( _setConsoleFunction ) {

		_setConsoleFunction( 'log', message, ...params );

	} else {

		console.log( message, ...params );

	}

}

/**
 * Enhances log/warn/error messages related to TSL.
 *
 * @param {Array<any>} params - The original message parameters.
 * @returns {Array<any>} The filtered and enhanced message parameters.
 */
function enhanceLogMessage( params ) {

	const message = params[ 0 ];

	if ( typeof message === 'string' && message.startsWith( 'TSL:' ) ) {

		const stackTrace = params[ 1 ];

		if ( stackTrace && stackTrace.isStackTrace ) {

			params[ 0 ] += ' ' + stackTrace.getLocation();

		} else {

			params[ 1 ] = 'Stack trace not available. Enable \"THREE.Node.captureStackTrace\" to capture stack traces.';

		}

	}

	return params;

}

/**
 * Logs a warning message with the 'THREE.' prefix.
 *
 * If a custom console function is set via setConsoleFunction(), it will be used
 * instead of the native console.warn. The first parameter is treated as the
 * method name and is automatically prefixed with 'THREE.'.
 *
 * @param {...any} params - The message components. The first param is used as
 *                          the method name and prefixed with 'THREE.'.
 */
function warn( ...params ) {

	params = enhanceLogMessage( params );

	const message = 'THREE.' + params.shift();

	if ( _setConsoleFunction ) {

		_setConsoleFunction( 'warn', message, ...params );

	} else {

		const stackTrace = params[ 0 ];

		if ( stackTrace && stackTrace.isStackTrace ) {

			console.warn( stackTrace.getError( message ) );

		} else {

			console.warn( message, ...params );

		}

	}

}

/**
 * Logs an error message with the 'THREE.' prefix.
 *
 * If a custom console function is set via setConsoleFunction(), it will be used
 * instead of the native console.error. The first parameter is treated as the
 * method name and is automatically prefixed with 'THREE.'.
 *
 * @param {...any} params - The message components. The first param is used as
 *                          the method name and prefixed with 'THREE.'.
 */
function error( ...params ) {

	params = enhanceLogMessage( params );

	const message = 'THREE.' + params.shift();

	if ( _setConsoleFunction ) {

		_setConsoleFunction( 'error', message, ...params );

	} else {

		const stackTrace = params[ 0 ];

		if ( stackTrace && stackTrace.isStackTrace ) {

			console.error( stackTrace.getError( message ) );

		} else {

			console.error( message, ...params );

		}
";

// C5 (WHY): the three.js loggers, constant fronts only, plus a front
// caught from a `try` (an unknown string: may hold `%j`).
#[test]
fn r9store2_c5_three_loggers_why() {
    let p = format!("{CLS}{THREE_LOG}\nerror( x, a ); warn( y, a ); log( z, a );\n");
    let on = runs(&p, "hit");
    eprintln!("R9STORE2 C5 const fronts hit runs={on}");
    let q = format!("{CLS}{THREE_LOG}\nerror( x, a ); try {{ JSON.parse( String( process.argv[2] ) ); }} catch ( e ) {{ error( e ); }}\n");
    let on2 = runs(&q, "hit");
    eprintln!("R9STORE2 C5 caught front hit runs={on2}");
}

// C6 RED: the rest logger with a later unknown String: only the FIRST
// argument is a format string (`formatWithOptions`), here the constant
// front `T.x`: the later String never makes `a` serialized. The call is
// fixed when it is made (`Gen::fixed_spreads`), before the console model
// reads it: before, the spread elements counted as a possible first.
#[test]
fn r9store2_c6_later_unknown_string_is_no_format() {
    let on = runs(
        &logger("log( \x27x\x27, a, String( process.argv[2] ) );"),
        "hit",
    );
    eprintln!("R9STORE2 C6 hit runs={on}");
    assert!(!on, "C6 toJSON body active");
}
