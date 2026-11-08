//! Element reads of ordinary Arrays (representation RFC §10.7, phase P4).

use super::*;
use crate::ElementOp;

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
fn zz_probe_board_read() {
    let pf = facts(BOARD);
    let offset = BOARD.find("this.board[square] ??").unwrap() as u32 + 1;
    eprintln!("READ {:#?}", pf.element(0, offset, ElementOp::Read));
    let woff = BOARD.find("this.board[square & 127]").unwrap() as u32 + 1;
    eprintln!("WRITE {:#?}", pf.element(0, woff, ElementOp::Write));
    let module = perry_parser::parse_typescript(BOARD, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&module, "t", "t.ts").unwrap();
    let text = format!("{hir:?}");
    if let Some(i) = text.find("fill") {
        eprintln!("HIR ...{}", &text[i.saturating_sub(600)..(i + 200).min(text.len())]);
    }
}
