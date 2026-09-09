# Fresh merged-main build and live GC witnesses

Pinned main `eee3881c464bf91ae900e87a42bed072bbdfc95a` (0.5.1529), after #10035.
The entire reviewed #10034 tree equals this merge. A new matched compiler,
runtime-static and stdlib-static release build passed in 5m20s. Original,
rotating and retained-lifetime worker objects were relinked against that archive.
All binaries and selected source inputs are hashed in provenance.json. Binary
hashes differ from the older corrected-R2 build, so measurements will use these
new workers explicitly. Inspected parse entry and depth-scanner code sizes and
addresses match corrected R2; exact codegen is archived without inferring whole
binary identity.

The protected scan performs 1323 copying minors and moves 208531 objects while
matching Node. Retained outputs match in normal, scheduled-moving and full-GC
modes. The 1000-parse Unicode diagnostic performs actual minor collections.
Stringify malloc-trigger cadence is unchanged at 39,40,40,40. Separate finite
escaped-record proof is archived in ../main-eee-finite-validation.

The matched build log retains existing warnings. This does not claim the full
repository CI passes: the inherited native-stack test and unrelated gap/snapshot
failures are tracked separately. Exact local link/GC drivers are historical
records; no fresh runtime-unit-test run is claimed by this build packet.
