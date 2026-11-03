## Runtime: admit native-alias holders and validate constructor transitions by shape

Native-backed subclass receivers retain ordinary slots and prototype links in
an explicit shape kind. Read-holder entries load current receiver slots and
retain both primary/saved own undefined slots and ordinary absence proofs for
native forwarding.
Ordinary prototype holders, deep method holders, and captured/rest/bound function
bodies can prime existing method sites. Split method lookup keeps its common
validator inline and decodes deep chains only for inherited entries.
Priming records the receiver shape before the method runs, so a method that
adds fields or enters dictionary mode can serve the next fresh receiver.
Prototype mutation, shadowing, deletion, accessors and Proxy chains retain their
shape guards or generic dispatch.

Constructor key-add entries retain their prototype-shape proof in the existing
transition words. Unrelated global prototype-validity changes no longer discard
an otherwise valid transition; each use guards its consumed holders and the
successor's field representation. ConstFn add entries retain their successor's
body, removing the overwrite-site latch from constructor admission. Different
bodies at the same pre-shape converge through the existing shape generalization
path. Empty literal builders reuse the plain-record birth routine, avoiding a
shape-cache lookup and recording ordinary stores before the first shape stamp.
Accessor literals can then cache additions to unrelated keys. All-Any successor
representations are immutable, so those entries reuse the existing unflagged
store arm without a representation load. The otherwise unused combination of
value flags retains guards for Any appends with typed prefix lanes. Short
prototype proofs use direct comparisons for their first three hops. Emitted hits
validate with direct loads and
comparisons, preserving their existing safepoint-free contract. Receiver-relative
alias slots share primary/saved holder decoding, leaving ordinary reads out of
the function-bag scan. Contiguous packed kind decoding retains a conservative
native-fallback result for reserved codes, avoiding a range-check ladder on
every valid shape read. The existing
root scanners trace and rewrite those entry-owned links. No independently
indexed cache, name check, side table
or latch is added.

Workers can inherit static shape ids. Generated add hits use the existing
worker gate before accessing primary-agent proofs; runtime add memos are
served only on the primary agent.

Declaration precedence for anonymous/class id collisions is projected when
class metadata is registered, removing name cloning and method enumeration from
shape minting. Membership and precedence are consumed in one metadata read,
removing the second anonymous-class lookup from the birth path. Differential gap cases, moving-GC checks, focused negative-control
witnesses and external miss-count/A-B drivers cover the mechanisms.
Integrated onto main d2ceafc938 with the shape-selected declaration origin and
S7 CLASS identity words retained. Declaration precedence uses the registered
name and published CLASS identity, including fieldless declarations and
classes whose members were deleted. Entry-owned constructor and method chain
proofs compare the live CLASS word and trace the saved holder through the
existing scanner. `prime_chain` also admits inherited spill slots. Landed readpath3 C1 changes
are retained; this lane adds none of its per-site keyed holder, WeakMap site,
sized literal or general read spill-entry work.

Allocation fast paths and the WeakSet brand are retained.
A direct-holder refusal distinguishes terminal shape-proven absence from a
chain that can still supply the method; failed terminal searches no longer
repeat the complete chain walk. Generic invocation entries retain a shape-owned
property slot without a body guard, so changing captured/bound method bodies
uses the existing entry instead of consuming its ways. Live closure checks,
direct-body guards and ConstFn shape guards remain.

Changing direct bodies at one ordinary data slot widens that same entry to
current-value invocation instead of consuming its ways. The value-format info
marker cannot match a valid body pointer, so ordinary direct-body hits retain
their original validation; the format check runs on their mismatch arm.
Generic invocation reuses the existing compiled-body dispatcher after live
closure validation, and clones captured this only when it differs from the
receiver. Arrows retain lexical this. The ubiquitous holder validator stays
inline. Constructor proofs share short-chain comparisons, with an exact
four-hop gate and a numeric SSA tail loop for deeper chains. This removes
duplicated guard blocks and per-site induction stack slots while preserving
every CLASS, prototype, worker and representation guard. Length-zero through
seven constructor fixtures and an isolated tail-induction negative control
cover the compact validator.

The pre-call prime runs entirely under collection suppression, whose exit
restores only the suppression flag. Method misses retain the original argument
buffer across that window instead of allocating handle and refreshed-value
vectors; the ordinary dispatch and captured-this rebind retain their existing
rooting contracts.

The bounded function-property scan stays out of line. Ordinary receivers keep
the inline shape-band eligibility test and class-site admission. Exotic
receivers return through one cold function/class query with the read front's
floating-point return ABI. The front no longer preserves its receiver, cache
and token across two queries, and LLVM can leave receiver-address materialization
on the holder path. Ordinary primary holder validation remains inline.
