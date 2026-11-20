//! `for...of` over a value typed `Set` or `Map` on the web target (the slot-cursor
//! fast path and its host cursors, `collection_cursor_next`).

use super::{run_web_source, run_web_source_then, run_web_source_with_dom, STUB_DOM};

// A `for...of` over a value typed `Set` or `Map` takes the slot-cursor fast path. It visits
// each entry once, sees entries deleted or added during the loop as Node does, and works
// nested, after a `break`, and in an async body.
#[test]
fn for_of_over_a_typed_set_or_map_visits_every_entry() {
    assert_eq!(
        run_web_source(
            r#"
function values(s: Set<number>) { let r = ""; for (const v of s) r += v; return r; }
function pairs(m: Map<string, number>) { let r = ""; for (const [k, v] of m) r += k + v; return r; }
function entries(m: Map<string, number>) { let r = ""; for (const e of m) r += e[0] + e[1]; return r; }
function deleting(s: Set<number>) { let r = ""; for (const v of s) { r += v; if (v === 1) s.delete(2); } return r; }
function adding(s: Set<number>) { let r = ""; for (const v of s) { r += v; if (v < 3) s.add(v + 1); } return r; }
function nested(s: Set<number>) { let r = ""; for (const a of s) for (const b of s) r += a + "" + b + " "; return r; }
function again(s: Set<number>) { let r = ""; for (const v of s) { if (v === 2) break; r += v; } for (const v of s) r += v; return r; }
function mapDeleting(m: Map<string, number>) { let r = ""; for (const [k] of m) { r += k; if (k === "a") m.delete("b"); } return r; }
async function inAsync(s: Set<number>, m: Map<string, number>) { let r = ""; for (const v of s) r += v; for (const e of m) r += e[0]; return r; }
const ab = () => new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log(values(new Set([1, 2, 3])) + " " + pairs(ab()) + " " + entries(ab()));
console.log(deleting(new Set([1, 2, 3])) + " " + adding(new Set([1])) + " " + mapDeleting(ab()));
console.log(nested(new Set([1, 2])) + "| " + again(new Set([1, 2, 3])) + " " + values(new Set()));
inAsync(new Set([4, 5]), ab()).then((v) => console.log(v));
"#
        ),
        "123 a1b2c3 a1b2c3\n13 123 ac\n11 12 21 22 | 1123 \n45abc"
    );
}

// Each Set/Map `for...of` owns its cursor: an inner loop over the same collection cannot
// move the outer one, a frozen collection iterates, a `continue` in the async body's JS
// fallback still advances the loop, and a loop left early releases its cursor.
#[test]
fn for_of_over_a_typed_set_or_map_keeps_a_cursor_per_loop() {
    assert_eq!(
        run_web_source_then(
            r#"
function nestedDelete(s: Set<number>) { let r = ""; for (const a of s) { r += a; for (const b of s) r += ""; s.delete(a); } return r; }
function mapNested(m: Map<string, number>) { let r = ""; for (const [k] of m) { for (const [j] of m) r += k + j; m.delete(k); r += " "; } return r; }
function values(s: Set<number>) { let r = ""; for (const v of s) r += v; return r; }
function skipping(s: Set<number>) { let r = ""; for (const v of s) { if (v === 2) continue; r += v; } return r; }
async function skippingAsync(s: Set<number>, m: Map<string, number>) { let r = ""; for (const v of s) { if (v === 2) continue; r += v; } for (const [k, v] of m) { if (v === 2) continue; r += k; } return r; }
const ab = () => new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
console.log(nestedDelete(new Set([1, 2, 3])) + " " + mapNested(ab()) + "| " + values(new Set([1, 2])) + " " + skipping(new Set([1, 2, 3])));
skippingAsync(new Set([1, 2, 3]), ab()).then((v) => console.log(v));
const exited = new Set([1, 2, 3]);
(globalThis as any).exited = exited;
function leave(s: Set<number>) { for (const v of s) if (v === 2) break; for (const v of s) if (v === 2) return v; try { for (const v of s) throw v; } catch (e) {} return 0; }
leave(exited);
"#,
            // `Object.freeze` has no web lowering, so the frozen case drives the bridges.
            r#"
const loops = __collectionCursors.get(globalThis.exited);
console.log(loops ? loops.size : "none");
const frozen = Object.freeze(new Set([7]));
const slot = __memDispatch.collection_cursor_next(frozen, 0, 0);
console.log(__memDispatch.set_value_at(frozen, slot));
"#
        ),
        "123 aaabac bbbc cc | 12 13\n13ac\n0\n7"
    );
}

// A Set/Map `for...of` left by `break`, `return` or `throw` calls its iterator's `return`;
// one that runs to the end does not. After a `throw`, the body's error wins over one
// thrown by `return`; after a `break`, the error from `return` propagates. The async
// bodies take the JS fallback.
#[test]
fn for_of_over_a_typed_set_or_map_closes_its_iterator_on_early_exit() {
    let tracked = format!(
        "{STUB_DOM}{}",
        r#"
globalThis.closeLog = "";
globalThis.failClose = false;
globalThis.Tracked = class extends Set {
  [Symbol.iterator]() {
    const it = this.values();
    return { next: () => it.next(), return: () => { closeLog += "R"; if (failClose) throw new Error("close"); return {}; } };
  }
};
"#
    );
    assert_eq!(
        run_web_source_with_dom(
            r#"
const g = globalThis as any;
function brk(s: Set<number>) { for (const v of s) if (v === 2) break; }
function ret(s: Set<number>) { for (const v of s) if (v === 2) return v; return 0; }
function thr(s: Set<number>) { for (const v of s) if (v === 2) throw new Error("body"); }
function all(s: Set<number>) { let r = 0; for (const v of s) r += v; return r; }
const s: Set<number> = new g.Tracked([1, 2, 3]);
brk(s); g.closeLog += " "; ret(s); g.closeLog += " "; all(s); g.closeLog += " ";
try { thr(s); } catch (e) { g.closeLog += (e as Error).message; }
g.failClose = true;
try { thr(s); } catch (e) { g.closeLog += " " + (e as Error).message; }
try { brk(s); } catch (e) { g.closeLog += " " + (e as Error).message; }
console.log(g.closeLog);
async function abrk(s: Set<number>) { for (const v of s) if (v === 2) break; }
async function athr(s: Set<number>) { for (const v of s) if (v === 2) throw new Error("async"); }
g.failClose = false;
g.closeLog = "";
abrk(s).then(() => athr(s)).catch((e) => console.log(g.closeLog + (e as Error).message));
"#,
            &tracked,
            "",
        ),
        "R R  RbodyR bodyR close\nRRasync"
    );
}
