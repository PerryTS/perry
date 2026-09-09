Fix discarded large JSON strings accumulating when parse completion overwrote
their pending malloc-sweep request. Preserve completed-byte debt until the
request is serviced, and retain accounting for bytes produced after a deferred
request so stringify does not admit an extra output between collections.
Collection remains outside JSON construction, with rooted inputs and completed
outputs.

Use the existing JSON construction batch for bounded lazy records after
consecutive reads, and for the adaptive whole-array reparse. Isolated sparse
reads retain their existing producer. Add output-lifetime and collection-cadence
regressions, rotating-input CPU/RSS comparisons, and reproducible benchmark
provenance for the merged baseline and follow-up candidates.

Give the lazy-record batch its own copied-minor test witness, so the regression
cannot pass by silently taking the legacy tape materializer. A disabled-batch
negative control now fails before the cache-store survival assertions.
