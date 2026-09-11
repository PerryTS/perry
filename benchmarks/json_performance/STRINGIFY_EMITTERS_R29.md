# R29 stringify safety and emitter investigation

**Not promoted or timed.** The candidate repairs the allocating-callback lifetime failure but exposes an inherited string-flag bug when its faster emitters encounter concatenated strings.

Source `f4d4ee8735f53700ac03320e8a33c273ca3c97e2` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71`; workspace 0.5.1531. Neither is current-main measurement.

## Findings

- The object replacer now roots the current property key and reloads its bytes after callbacks. It reuses one handle per object and scopes each replacer pointer at its call.
- Pretty/replacer scalar paths use the existing heap and short-string emitters. A parsed heap string concatenated with a newline or quote inherits the escape-free bit through a flags union. The fast writer then copies unescaped bytes into JSON.
- All 20 candidate executions exit normally and complete the 64-retained-output callback check, including protected moving GC. Fourteen complete outputs nevertheless differ from Node in the mutation rows; six forced-tape controls pass.
- The unchanged reference has six allocating-callback failures (auto/direct, native/shadow, including depth-64 SIGBUS) and 14 passes. Its native/shadow static root checks pass, demonstrating why runtime callbacks and byte-for-byte output checks are required.
- The next revision clears escape-free provenance on concatenation and in-place appends while preserving lone-surrogate metadata. It adds plain stringify to the mutation fixture and targeted runtime tests. That revision is separate from this frozen source.

## Executed validation

Not promoted. 295 serial release JSON unit tests pass, but the new emitted-output fixture exposes inherited escape-free flag propagation when parsed strings are concatenated with quotes/newlines. All 20 candidate processes return 0 and validate all 64 retained callback outputs; 14 full outputs differ from Node in earlier mutation rows. Six forced-tape controls pass. Reference has 14 passes and six allocating-callback failures. The original 81 candidate controls and candidate IR checker were not run because new behavior validation failed first. The 81 copied original reference receipts are not fresh candidate evidence. Reference emitter native/shadow static checks pass.

The final f4 source completed a clean, normal all-three-package production build in 545.522 seconds; immutable artifacts and copy/worktree provenance are retained. Initial db78 unit tests passed, but its production attempt overlapped a source correction and was explicitly terminated/discarded; it was never linked into a measured worker. Final lint73/74 passes; public benchmark freshness fails; filecap passes; compile-tier and two CI-only lint gates skipped. No R29 remote timing window, staging or PR.

## Build fingerprints

| Artifact | R26 SHA-256 | R29 SHA-256 |
|---|---|---|
| perry | `794b7dfa67f5f3509f96ddbeeff3cf9e70c1bb4efc0383cd467f90bf2d662040` | `0895d6d33b6413670e5173cb9d9218f33f4b616979806ebe188aaa40b23fd269` |
| libperry_runtime.a | `48972667fc2bf53e57c2776eb8741e32e41d1da752017abbe2aa512325af23bb` | `5ae8a205668f1fc5b5ab160be0d2caac64249f0eeae9a86172a0a799d2bcc34b` |
| libperry_stdlib.a | `1b343ca0329e233c477688597c452ac9750100b89675ec012336c96a9632917c` | `bdcd4034356a9690b92fa12d84ebc67c6df97caf44ace9b75bb76d52186bec0e` |

## Independent access investigation

The archived R26 access profile is diagnostic evidence, independent of R29. fmod accounts for 11.74% of 690 field-walk samples and 51.44% of 731 random-read samples, underneath workload index arithmetic through js_dynamic_mod. The complete Node loop checksum and KEEP match. Inclusive groups overlap; sampled process CPU/RSS is not timing evidence. Most remaining field-work self samples are generated code, so zero named property/GC frames do not imply zero inlined cost.

A proposed exact nonnegative-u32 remainder arm passed 8,150,393 standalone numeric cases (4,065,390 admitted; 4,085,003 fell back), with signed-zero bit equality and NaN-aware comparison. This is a model, not production integration or a speed claim. Source, compiler/binary hashes and output are archived.
