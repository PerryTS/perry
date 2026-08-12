**The raw-handle debt ratchet now holds across a pull request boundary.**
CI compared the counted `get_raw_{mut,const}_ptr` sites against
`scripts/raw_handle_debt_baseline.txt` *from the pull request checkout* — a
number the same diff was free to move, so adding bare reads and raising the
baseline (and the per-module ceilings) to match passed the gate. The guarded
`--update` path refuses to raise, but nothing made CI run it. A new
`--no-raise-vs <ref>` reads both recorded files out of the merge base and fails
if the checked-out copies are larger anywhere — the total, an existing module's
ceiling, or a module absent from the base's list, which is a raise from zero
rather than a fresh start. Unchanged and lower both pass. Because an unfetched
merge base makes every file read as absent, and that is indistinguishable from
"the gate did not exist there", the ref is resolved *before* any file is read
and an unresolvable one fails the build rather than comparing against nothing;
the workflow gates the step on `github.event_name == 'pull_request'` rather than
on an empty variable. `--self-test` grew eight cases covering all three raises,
four legal diffs, and the unresolvable ref. (#7659, reported in #7389)
