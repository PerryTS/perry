perry now reads the runtime call-effect table from the `.gc-effects` sidecar
shipped beside the prebuilt runtime archive it links (GC effects design 4.3,
slice 6). The sidecar's SHA-256 key is checked against the archive bytes on
every compile, and the linked combination's section is joined per row with
the committed table (the weaker row wins). A missing or stale sidecar is a hard
error naming `scripts/gc_call_effects/sidecars.sh <dir>` when perry runs from
its workspace; an installed perry compiles with every runtime call treated as
a safepoint that can throw and prints one warning. Every recipe that builds the
runtime archives and then compiles through perry now writes their sidecars.
