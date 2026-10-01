**Objects:** Inherited reads and method calls now use receiver and holder shape facts; the old inherited-read cache and its GC roots are removed. Class getter and setter sites cache direct accessors with live shape and prototype checks. Worker startup gates holder-backed sites so each agent uses ordinary dispatch safely.

Reject class accessor cache entries whose closure getter has no compiled getter, even when a compiled setter remains; both priming and cache hits fall back to generic getter dispatch.
