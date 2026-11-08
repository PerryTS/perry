//! Element reads of ordinary Arrays whose storage the program never moves
//! (`bindings::ElementRead`, representation RFC §10.7, phase P4).

use crate::ElementRead;

fn reads(src: &str, at: &str) -> Option<ElementRead> {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &Default::default());
    let mb = pf.bindings_for("t.ts").expect("module bindings");
    // SWC offsets are 1-based; the read's offset is its object's start.
    let offset = src.find(at).expect("read in source") as u32 + 1;
    mb.element_reads
        .iter()
        .find(|(o, _)| *o == offset)
        .map(|(_, r)| *r)
}

const BOARD: &str = r#"
class Board {
  private board: number[] = new Array<number>(128).fill(0);
  private at(square: number): number { return this.board[square] ?? 0; }
  put(square: number, piece: number): void { this.board[square & 127] = piece; }
  load(): void { const next = new Board(); this.board = next.board; }
  count(): number {
    let n = 0;
    for (let s = 0; s < 128; s++) if ((s & 0x88) === 0 && this.at(s) !== 0) n++;
    return n;
  }
}
const b = new Board();
b.put(3, 4); b.load(); b.put(5, -2);
console.log(b.count());
"#;

#[test]
fn a_fixed_length_field_array_read_is_proven() {
    let r = reads(BOARD, "this.board[square] ??").expect("proven read");
    assert_eq!(r.len, 128);
}

#[test]
fn a_literal_array_read_in_bounds_needs_no_bound() {
    // A loop counter is narrowed in a function body (`gen::narrow`).
    let src = "const a = [1, 2, 3, 4];\n\
               function sum(): number {\n\
                 let s = 0;\n\
                 for (let i = 0; i < 4; i++) s += a[i];\n\
                 return s;\n\
               }\n\
               console.log(sum());";
    let r = reads(src, "a[i]").expect("proven read");
    assert_eq!(
        r,
        ElementRead {
            len: 4,
            key_in_bounds: true
        }
    );
}

#[test]
fn an_array_that_may_grow_is_not_proven() {
    let src = "const a = new Array<number>(4).fill(0);\n\
               let s = 0;\n\
               for (let i = 0; i < 4; i++) s += a[i];\n\
               a.push(5);\n\
               console.log(s, a[4]);";
    assert_eq!(reads(src, "a[i]"), None);
}

#[test]
fn an_index_store_past_the_length_is_growth() {
    let src = "const a = new Array<number>(4).fill(0);\n\
               function put(k: number) { a[k] = 1; }\n\
               put(Math.floor(Math.random() * 10));\n\
               console.log(a[1]);";
    assert_eq!(reads(src, "a[1]"), None);
}

#[test]
fn an_array_given_an_element_descriptor_is_not_proven() {
    let src = "const a = [1, 2, 3];\n\
               Object.defineProperty(a, 1, { get() { return 7; } });\n\
               console.log(a[1]);";
    assert_eq!(reads(src, "a[1]"), None);
}

#[test]
fn an_array_reached_through_a_global_is_still_the_programs() {
    // The program is closed: a global store is one more store it sees.
    let src = "const a = [1, 2, 3];\n\
               (globalThis as any).leak = a;\n\
               console.log(a[1]);";
    assert_eq!(
        reads(src, "a[1]"),
        Some(ElementRead {
            len: 3,
            key_in_bounds: true
        })
    );
}

#[test]
fn a_receiver_that_may_be_another_object_is_not_proven() {
    let src = "const a: any = Math.random() < 0.5 ? [1, 2, 3] : { 1: 'x' };\n\
               console.log(a[1]);";
    assert_eq!(reads(src, "a[1]"), None);
}

#[test]
fn a_key_outside_the_length_keeps_its_bound() {
    let src = "const a = [1, 2, 3, 4];\n\
               function sum(): number {\n\
                 let s = 0;\n\
                 for (let i = 0; i < 8; i++) s += a[i] ?? 0;\n\
                 return s;\n\
               }\n\
               console.log(sum());";
    let r = reads(src, "a[i]").expect("proven read");
    assert_eq!(
        r,
        ElementRead {
            len: 4,
            key_in_bounds: false
        }
    );
}
