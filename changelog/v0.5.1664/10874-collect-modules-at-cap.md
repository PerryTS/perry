Allowlisted `crates/perry/src/commands/compile/collect_modules.rs` in
`scripts/check_file_size.sh`.

The file sat at **exactly 2000 lines on main** — the cap, with zero headroom —
so the next PR to add any line to it fails the required `lint` gate. That is
what caught #10874 and #10867's module-resolution work, which between them add
one line.

Allowlisted rather than split, matching the `#1435` block's stated rationale
("allowlisted here to unblock the required lint gate rather than fold an
unrelated refactor into an in-flight PR"). The reason it applies here is
structural: `collect_module_one` is a single ~1,890-line walk spanning lines
111–1999, beneath **15 already-peeled sibling modules** (`walk`, `worker`,
`json_module`, `native_addon`, `reexport_prune`, …). The easy arms have already
been lifted out; what remains is one function, so the next split is phase
surgery on a walk, not a move of independent arms — and it does not belong
folded into a resolution fix.

Worth recording as a general hazard: **a file trimmed to exactly the cap is a
landmine.** It passes on the commit that trims it and fails for whoever touches
it next, with the cost landing on an unrelated PR. A split should leave real
headroom — this train's `object/mod.rs` split went to 1979, and the earlier
`descriptor_state.rs` split moved 264 lines rather than the minimum 42, for the
same reason.

Follow-up: peel `collect_module_one` into phase submodules.
