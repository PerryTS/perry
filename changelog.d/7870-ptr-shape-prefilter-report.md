**`--opt-report` now reports boxed and module-global `Ptr<Shape>` candidates instead of silently omitting them** (#7112).

These `let`-bound object allocations are rejected before the containment walk, so the report previously could not distinguish them from values the analysis never examined. Reporting builds now record the provenance denial while ordinary builds keep the existing single scan.
