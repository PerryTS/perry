Primitive inline objects reuse the general walker's field preflight, input
root, prototype handling and property order. Once that preflight rules out
callbacks, the emitter borrows the keys and values once instead of retrieving
them repeatedly through the root handle. Output uses the same scalar encoders.
Descriptors, class instances, overflow, nested values, BigInt and raw-pointer
shapes retain the general traversal. No new managed scratch, GC scheduling or
production GC policy changes are included.

138 JSON-filter runtime tests and 33 compiled Node comparisons pass. The new
compiled wide-object witness also matches Node on the preceding release; it
covers property ordering, retained output and mutation, late getters and
non-enumerability, nested toJSON results, default/custom prototypes, a replacer
and spacing. A unit test emits 128 fields 1000 times without managed growth,
collection or added handles. The root-holder inventory retains its prior
unverified frontier. All 24 moving-GC stress runs match Node and show scheduled collections,
copying and live object movement. Qualified CPU/RSS results are pending.

The local instruction diagnostic is promising on wide-object stringify, but
no all-row or no-regression acceptance follows from it. The last qualified
standings remain those of ../empty-object-leaf.
