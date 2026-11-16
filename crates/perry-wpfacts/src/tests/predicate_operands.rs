//! A predicate that converts nothing (`Array.isArray`, `Object.is`,
//! `Boolean`, `Number.isNaN` / `isFinite` / `isInteger` / `isSafeInteger`)
//! runs no ToPrimitive on its operand (ECMA-262 23.1.2.2, 20.1.2.13, 7.1.2,
//! 21.1.2.2-21.1.2.5): a function stored as the operand `valueOf` is never
//! called with it as the receiver (three.js `Object3D.toJSON`:
//! `Array.isArray( this.material )`). `isNaN` ToNumbers (19.2.3) and stays
//! the control.
use super::*;

fn prog(test: &str) -> ProgramFacts {
    facts(&format!(
        "class Mat {{
           name: string = \"m\";
           valueOf: any = 1;
         }}
         const mat: any = new Mat();
         mat.valueOf = new Function(process.argv[2]);
         console.log({test}, mat.name);"
    ))
}

#[test]
fn predicate_operands_convert_nothing() {
    let pf = prog(
        "Array.isArray(mat), Object.is(mat, 1), Boolean(mat), Number.isNaN(mat), \
         Number.isFinite(mat), Number.isInteger(mat), Number.isSafeInteger(mat)",
    );
    assert_eq!(tmpl(&pf, "Mat").escape, None, "a non-converting predicate called valueOf");
    // Control: isNaN ToNumbers its operand: valueOf is called on mat.
    let pf = prog("isNaN(mat)");
    assert!(tmpl(&pf, "Mat").escape.is_some(), "isNaN must call the unknown valueOf");
}
