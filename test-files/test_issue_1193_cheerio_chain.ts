// Issue #1193 — cheerio chain through any-typed intermediates.
//
// Pre-fix, `load(html).select(".x").text()` worked for direct chains but
// any user-side intermediate landed a SelectionHandle in a typed-as-number
// `let` binding, and the follow-up `.text()` then fell through codegen's
// static cheerio dispatch and surfaced as
// `(number).text is not a function`. Same shape for `(load(html) as any)`
// or anywhere TS-side type info evaporated.
//
// Fix (perry-stdlib/src/cheerio.rs + common/dispatch.rs): added a
// runtime-side `dispatch_cheerio` arm to `js_handle_method_dispatch` that
// routes both `CheerioHandle` and `CheerioSelectionHandle` methods to
// their corresponding FFI calls. Now the chain reassembles via the
// runtime fallback whenever the codegen lost the static cheerio type.
//
// The user-facing `$(sel)` callable form (the original repro shape) is a
// separate codegen-level fix tracked in the same issue — runtime dispatch
// alone can't make a non-closure handle callable. This regression covers
// the achievable slice: the documented `.select(sel)` method form must
// survive any-typed bindings.
import { load } from "cheerio";

const $ = load("<html><body><p class='x'>hello</p><p class='x'>world</p></body></html>");

// The any-typed intermediate is what regressed before the dispatch fix.
const sel = $.select(".x");
console.log("text:", sel.text());

// Same shape from a doc bound through a `let` — the document-level
// `.select()` arm of dispatch_cheerio is what closes this one.
const doc = $;
const sel2 = doc.select(".x");
console.log("text2:", sel2.text());
