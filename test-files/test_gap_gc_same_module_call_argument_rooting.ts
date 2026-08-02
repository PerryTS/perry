// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_ZEAL=1
//
// #7154: a call to a top-level function in the SAME module must root its
// arguments, exactly as the cross-module call #7240 fixed does.
//
// THE `parity-env` LINE IS THE TEST — see the same note in
// `test_gap_gc_rest_argument_rooting.ts`. Measured on the parent (`6aeef5baf`):
// in the harness's default configuration the broken compiler prints `bad 0`
// 10/10 and this file gates nothing; with the line it SIGSEGVs 10/10.
//
// This is the follow-up #7240 named and could not fold in. Its own regression
// test needed a two-file fixture precisely because a same-file callee does not
// take `lower_call/extern_func.rs`'s path at all — it resolves through
// `Expr::FuncRef(fid)` into `lower_call/func_ref.rs`, which had the identical
// defect one `else` away, in four arms rather than one:
//
//     } else if …synthetic `arguments` && rest…  { for a in args { lower } … }
//     } else if …synthetic `arguments`…         { for a in args { lower } … }
//     } else if has_rest                        { for a in args { lower } … }
//     } else                                    { for a in args { lower } }
//
// Every one lowered its arguments into bare SSA registers and then held them
// across work that allocates — the rest arms across `js_array_alloc` plus a
// `js_array_push_f64` per element, the plain arm across the later arguments'
// own lowering.
//
// Why this was not simply copied from #7240: `func_ref.rs` threads `lowered`
// through FOUR specialized-ABI dispatch paths (Tier A static, Tier B guarded,
// and the typed-f64 / i32 / string / i1 clones), each a fast/fallback diamond
// with a phi at the merge. The temp-root release has to sit in the merge block
// that post-dominates all five call sites — releasing on one side of a diamond
// leaves the other side's call reading dropped slots. That is a real change,
// not a one-line copy, which is why #7240 named it instead of guessing at it.
//
// Both protections are exercised, as in #7240: a STRING LITERAL argument is
// `OperandProtection::Reload` (its `__perry_init_strings_*` handle global is a
// registered root that an evacuating cycle REWRITES, so re-emitting the load
// below the collection point is correct and free), and a LOCAL argument is
// `OperandProtection::Root` (re-deriving it could observe an assignment made
// after the call-time value was taken, so it takes a real temp-root slot).
//
// LIVE BY CONSTRUCTION: `churn` keeps allocating AFTER the back-edge poll that
// collects, so the retired from-space bytes are recycled before the callee
// reads them and a stale read returns wrong text rather than the right answer
// out of memory nobody has reused yet.

// Allocates hard, and keeps allocating after the poll that collects.
function churn(n: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 200; i++) {
    bits.push({ i: i, s: "y" + i, pad: [i, i + 1, i + 2] });
  }
  return bits.length === 200 ? n : -1;
}

function freshUrl(i: number): string {
  return "/v0/orgs/" + i + "/full-scans/[full_scan_id]";
}

// SAME-MODULE callee, no rest: `func_ref.rs`'s plain `else` arm — the direct
// twin of the cross-module arm #7240 fixed.
function joinSame(
  url: string,
  method: string,
  opts: { n: number },
  schemaTag: number,
  parseTag: number,
): string {
  return url + " " + method + " " + opts.n + " " + schemaTag + " " + parseTag;
}

// SAME-MODULE callee WITH rest: `func_ref.rs`'s `has_rest` arm. Two fixed
// params plus a rest means the fixed params are held across the whole
// rest-array construction, and the accumulator holds the only reference to
// everything pushed so far while the next argument is lowered.
function joinSameRest(
  url: string,
  method: string,
  ...tags: number[]
): string {
  let total = 0;
  for (let i = 0; i < tags.length; i++) {
    total += tags[i];
  }
  return url + " " + method + " " + tags.length + " " + total;
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 8; r++) {
    // --- the plain arm ------------------------------------------------------
    // Reload: both string operands are literals, the registry's exact shape.
    const litOut = joinSame(
      "/v0/orgs/[org_slug]/full-scans",
      "GET",
      { n: churn(r) },
      churn(r),
      churn(r),
    );
    if (litOut !== "/v0/orgs/[org_slug]/full-scans GET " + r + " " + r + " " + r) {
      bad++;
    }
    // Root: argument 1 is a local holding a freshly-allocated string, which is
    // always young and therefore moves on every evacuating minor.
    const url = freshUrl(r);
    const freshOut = joinSame(url, "POST", { n: churn(r) }, churn(r), churn(r));
    if (freshOut !== url + " POST " + r + " " + r + " " + r) {
      bad++;
    }

    // --- the rest arm -------------------------------------------------------
    const litRest = joinSameRest(
      "/v0/orgs/[org_slug]/full-scans",
      "GET",
      churn(r),
      churn(r),
      churn(r),
    );
    if (litRest !== "/v0/orgs/[org_slug]/full-scans GET 3 " + 3 * r) {
      bad++;
    }
    const url2 = freshUrl(r);
    const freshRest = joinSameRest(url2, "POST", churn(r), churn(r), churn(r));
    if (freshRest !== url2 + " POST 3 " + 3 * r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
