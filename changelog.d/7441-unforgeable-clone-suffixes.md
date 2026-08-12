Fixed #6927: user members whose names spell a generated clone suffix
(`foo__generic`, `add__typed_f64`, `tick__pshape`, `A__dup1`, …) no longer
collide with the compiler's clone symbols. The failure had silently worsened
since the issue was filed: `deduped_function_refs` (first-define-wins, added
for minified same-name classes) swallowed the duplicate definition, so the
user function's public entry was usurped by its sibling's clone and indirect
calls executed the wrong body (`const g = add__typed_f64; g(2, 3)` returned
`add(2, 3)`). Generated suffixes now use a reserved `$` separator
(`{public}$generic`, `$typed_*`, `$spec_<reps>`, `$pshape`, `$dupN`);
`sanitize`/`sanitize_member` output is strictly `[A-Za-z0-9_]`, so no
user-derived symbol can compose a clone symbol, by construction, for every
current and future clone kind — including the cross-component shapes (class
`C__foo` + method `pshape`) that member-name checks cannot see. The #6925
`__pshape` collision prune is dead under this invariant and was reduced to
its registry-presence filter (`prune_unregistered_clones`); both symbol
reachability ratchets track the new spellings with unchanged allowlists.
Regression coverage: `codegen/clone_suffix_tests.rs` (emitted-IR contract,
per-PR visible), `helpers::sanitize_tests` (no-`$` mangling invariant), and
gap test `test_gap_6927_clone_suffix_user_members.ts` (direct + indirect
calls across the family, byte-identical to Node).
