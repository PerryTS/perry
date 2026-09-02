### Fixed

- **`o[k] += 1` on an ordinary object could hand the setter the RECEIVER as its
  key and SIGSEGV** — the second witness for #9499, fixed by #9522's root-reload
  launder. The held-out `test_gap_9459_property_set_strictness.cts` case goes
  back in as the regression pin.

  Reported as #9542: adding a frozen class instance whose field is named
  `caller` to that fixture made the module segfault on an *earlier* statement,
  `computedPlus[computedKey] += 1`, inside `set_field_by_name_object_tail` while
  it hashed the key. The issue's three leads were all wrong, and each is worth
  recording because each looks right:

  - **`caller` is not load-bearing.** Renaming the field to `zzz`, writing it
    with `=` instead of `+=`, or never writing it at all reproduces the crash
    byte-identically. Any perturbation of the module's object-literal shape set
    does. The `caller`/`arguments` poison-pill special cases in
    `field_set_by_name.rs` and `field_set_by_name/write_helpers.rs` are not on
    the path.
  - **It is not a rooting gap in `expr/index_set.rs`.** The
    `with_operands_rooted_across(ctx, &[object, index], &[value], …)` group is
    present and correct, and the emitted IR for the crashing statement is
    structurally identical with and without the added class — only SSA
    numbering and two `AnonShape` hashes differ.
  - **It is not a stale interned-key handle or a string-pool collision.** The
    key handed to `js_object_set_field_by_name` was neither stale nor
    mis-interned: it was the receiver. Instrumenting
    `js_get_string_pointer_unified` caught it taking the `POINTER_TAG` branch on
    `0x7ffd_046b_fceb_06f0`, and the setter then hashed the receiver's
    `parent_class_id` word as a `byte_len` of `0x800011bc` and walked off the
    end of the heap.

  The real mechanism is #9499's: `load %root_slot` folded back into an earlier
  safepoint's spill slot, which had been recycled in the hole, so the reload
  produced a different live object. A GDB watchpoint on the machine slot backing
  the key shows the last write before the faulting read storing the receiver.
  Proved by A/B on Linux x86-64 with one variable: the `#9459` branch
  (`9f40f65f5`) SIGSEGVs on this fixture, and the same tree with **only**
  `c405ce4981` (#9499 → #9522, `ROOT_RELOAD_LAUNDER`) cherry-picked runs it
  clean, three for three. Current `main` already carries that commit; #9542 is
  fixed there and needs no further code change.

  - `test-files/test_gap_9459_property_set_strictness.cts` — restores the case
    the file held out with a comment pointing at #9542: a `CallerCell` class
    whose field is literally named `caller`, frozen and written with `+=` in
    **both** arms, plus the unfrozen control that proves the store still lands.
    A field named `caller` on an ordinary object is not the poison pill — that
    accessor lives on `Function.prototype` and is keyed on the receiver — so
    this must behave exactly like the `Cell` case beside it: silent in sloppy
    code, `TypeError` in strict, value unchanged either way. Reverting #9522's
    launder turns it back into a SIGSEGV with the arm's output truncated
    mid-run, so it fails for the reason it was added.
