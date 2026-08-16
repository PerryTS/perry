### perf(codegen, runtime): the parameter validator stops recording visits it can never consult

`js_param_type_guard` runs on every unproven call into a guarded
ordinary-parameter clone. #8201 routed the scalar descriptors to the typed-abi
leaf guards; what stayed on the interpretive validator is the structural half,
and #8202 priced its **fixed** per-call component. Two of those three costs are
removable without changing what the guard decides.

**The visited set was unconditional.** Every container the walk touched paid a
linear scan of up to 64 inline entries and, past that, a `HashSet` insert — per
ARRAY ELEMENT. Validating `p: { toks: Token[], pos: number }` on every `peek(p)`
therefore recorded one entry per token, and not one of them could ever be
consulted: `Token` lies on no descriptor cycle and is reachable by exactly one
path, so a second arrival at the same `(address, node)` pair is impossible.

The set is load-bearing for exactly two facts, and both are properties of the
immutable compiler-emitted graph rather than of the value being validated:

* **termination** — a value cycle (`env.parent === env`) can only walk forever
  through a node that reaches itself;
* **no re-walk blowup** — a node the traversal can enter twice with the same
  address must memoize, or a shared graph re-walks exponentially.

So the compiler decides it. `visit_tracking_bits` runs Tarjan over the graph it
just built and propagates a saturating "ways in" count from the root, then sets
the high bit of the op byte on exactly the container nodes that need recording;
the runtime masks the op byte and reads the bit instead of recomputing the
answer per call. On `interp.ts`: `peek(p: Parser)` — 6 nodes, **0 tracked**,
where every token used to be recorded; `asNum(v: Value)` — 123 nodes, **15
tracked**, exactly the recursive `Node`/`Env` cluster. Descriptor length is
unchanged; the bit rides in a byte that only ever held ops 0–16. The magic goes
`PGT1` → `PGT2` so a mismatched compiler/runtime pair fails closed on the magic
(guard returns 0, caller takes the generic function) rather than reading a v1
blob as one that opts out of tracking everywhere.

**`GuardState` zeroed 1 KB of stack per call.** `inline_visited` is now
`MaybeUninit`; only `[..inline_visited_len]` is ever read, and after the change
above most guarded calls never write a slot at all.

Measured on the 19-program corpus (instructions retired, best-of-3, stdout
byte-exact, `iso_miss` still `misses 0`, both arms built from their own tree
with the same `-p` set and `PERRY_RUNTIME_DIR` pinned per arm). **Exactly 2 of
the 19 rows emit a `js_param_type_guard` call site** — `interp` and `iso_miss`,
two each (`asNum`, `peek`) — and both improve: `interp` **−1.63%**, `iso_miss`
**−1.36%**, peak RSS unchanged on both. Differencing against the same runtime
archive so binary-layout effects cancel (a `PGT1` blob under a `PGT2` runtime
fails every guard), the validator's own cost falls `interp` 1.671 B → 1.422 B
(**−14.9%**, 12.05% → 10.45% of the program) and `iso_miss` 1.611 B → 1.400 B
(**−13.1%**, 9.76% → 8.60%).

★ The other 17 rows are **not attributable in either direction**. The two arms'
`libperry_runtime.a` differ in exactly two functions out of 11,185 in the
crate's codegen unit — `js_param_type_guard` (808 → 316 bytes) and
`GuardState::matches` (+28) — with every other function byte-identical, and
those 17 programs execute neither. Their movement (`pipeline` −3.9%,
`retain_wide1` +0.6%, `deeplist` −0.5%, the rest within ±0.1%) is address-layout
noise: two `main` builds from identical source came out byte-identical (archive
and `perry` binary alike) and repeat runs of one binary spread ~0.1%, so the
build is deterministic and `pipeline`'s ±4% is what an address-hash-sensitive
program does when the heap moves. None of it is claimed here.

★★ #8202's premise — that the fixed per-call overhead dominates — does not hold.
It is ~15% of the validator's cost; the structural walk is the other ~10.5pp of
`interp`. Measured separately (see the issue), a diagnostic runtime whose guard
always accepts and one whose guard always rejects land within 0.3% of each
other, 12% below `main`: on these two rows the specialization the validator
gates is worth ~0.2% while running the validator costs ~12%. That is a policy
question for #8094/#8079, not a per-call-overhead one.
