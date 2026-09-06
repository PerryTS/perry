Empty-object stringify can return its inline result before creating a field
plan or a temporary runtime handle. It refreshes the prototype verdict on each
call and requires the no-allocation probe; a cold lookup declines to the rooted
general serializer. Nonempty objects retain their input root before lookup and
before output allocation. No GC production policy changes are included.

135 runtime JSON-filter tests pass, including a forced-moving cold-empty-input
fallback and the prior nonempty first-lookup witness. 1000 warm leaf calls show
no managed growth or collection. All 32 compiled Node comparisons and 22
moving-GC stress runs pass. The new compiled witness also matches the preceding
runtime: prototype methods/getters, deletion, explicit/null prototypes, hidden
toJSON, replacers, spacing, mutation and retained output remain observable.

The exact release binary is staged for comparison. No CPU/RSS measurements
for this source have started yet; the last qualified matrix remains the
preceding data-record release. This checkpoint does not establish parity or
no-regression acceptance.
