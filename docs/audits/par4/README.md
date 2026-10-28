# Private own-slot regression (PAR4 / PAR5)

PAR4's audit at `9c1e014b338df686d94db5e838542e23594add51` identified a whole-program precision loss: an escaped record invalidated a private record's own-field read because both records shared a layout. The original audit and source-isolation receipts are preserved in `/root/PAR4/par4.bundle`, commit `fc5078efd9678f9c9cb42113c5de46cd207fe074`. The diagnostic source edits from that audit are not compiler rules or benchmark changes.

PAR5 turns its runnable fixture into a regression test for DEC220. A read-site certificate checks every solved receiver object. An escaping or reconfigured sibling does not invalidate that object's definite own data slot. Template-wide facts still meet every birth, and remain conservative for contracts and other consumers. The rule is applied in whole and per-unit solves without assuming unit closure.

The certificate refuses unknown, empty, escaped, or modified receivers, foreign templates without a sufficient contract, and any applicable global, prototype, or accessor obstruction. Literal-shape prototypes are layouts rather than runtime objects (DEC217(a)); an escape attributed to that synthetic layout alone is not a runtime alias to the private receiver. Real prototype objects, HARD operations and accessor obligations remain checked. The recovered certificate proves one own slot, not a fixed full layout, field value kind, or method target.

## Run the regression and escape control

From the repository root, select a built compiler and its matching runtime:

```sh
PERRY_BIN=/path/to/perry node docs/audits/par4/precision-probe.mjs
PERRY_BIN=/path/to/perry node docs/audits/par4/precision-probe.mjs --require-whole
PERRY_BIN=/path/to/perry node docs/audits/par4/precision-probe.mjs --sabotage
```

Both ordinary commands require whole and per-unit proofs. `--require-whole` remains accepted for the original audit command. On the old compiler the whole assertion fails after successful compilation and a live-site check. `--sabotage` gives the private receiver itself to unknown code; its unchanged per-unit assertion must fail. A compilation error is never a valid negative receipt.

The fixture also executes JavaScript with an actual mutator of the sibling record and requires output `2`. Thus an isolated layout is not being substituted for the confinement argument, and actual escape is not excused.

The wpfacts unit tests in `tests/receiver_slots.rs` cover sibling escape, deletion, data-property reconfiguration and prototype changes; actual alias, field and prototype paths to the private receiver; receiver deletion, getter reconfiguration and prototype mutation; and named/computed accessors on `Object.prototype`. They assert that template contracts retain the escaping sibling.

## Array callback precision

The NavigateTo audit's unrelated builtin-function receivers came from specialized array operations using computed-key reads for numeric elements. In particular, `ArraySort` and `ArraySlice` admitted array methods as values passed to comparator/map callbacks. PAR5 uses the existing element-read rule for those operations and the corresponding copy, flatten, reduce, pop/shift and `at` operations. The solved receiver decides whether the element rule applies; other receivers retain full lookup. Types do not establish the receiver's semantics.

`tests/array_numeric.rs` covers record callbacks through sorting and copies, an actual builtin-function element, and a copied array whose elements really escape. The latter two controls must continue to refuse unconditional record-field proofs.

## Production audit

PAR5's report records the exact original source identities and before/after results for W-NAV, W-CELL, W-CLASSIFY, W-COMPLETE, W-TARGET and W-RANGE. Conditional E0 entry results and whole bodies with no callee flow remain separately named residuals, as directed in the DEC220 ruling. Raw-copy claims refused by codegen's weakest-copy meet remain separate too. They are not unconditional claims about unknown or empty receivers.

The full report, source/copy inventories, CWREF template comparisons, verification receipts and startup comparison are delivered under `/root/PAR5/`. No source-offset fallback or ordinal template pairing is used.
