`{ ...a, ...r }` where `r` came from `{ ...undefined }` crashed in
`spread::birth_slow`: the prefixed path read the source's key list
unconditionally, and a keyless layout's list is null. `copy_slots` had the
same read. marked 18 does exactly this on every `marked.parse(src)` without
options (`let r = { ...s }, o = { ...this.defaults, ...r }`), so it segfaulted
(competitive npm-heavy). Both reads now skip a source with no keys, as
`leading_slow` already does.
