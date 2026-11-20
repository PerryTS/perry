**Objects:** Inherited reads and method calls now use receiver and holder shape facts; the old inherited-read cache and its GC roots are removed. Class getter and setter sites cache direct accessors with live shape and prototype checks. Worker startup gates holder-backed sites so each agent uses ordinary dispatch safely.

Reject class accessor cache entries whose closure getter has no compiled getter, even when a compiled setter remains; both priming and cache hits fall back to generic getter dispatch.

Use genuine rooted GC objects as pointer keys in the ordered-delete Map test,
and reload the Map and key addresses after the allocating phase. Native Rust
boxes do not carry the GC headers inspected by Map key classification.

Honor relinked declared-class prototype chains for inherited reads and preserve the original getter receiver. Keep the Windows Inkwell 0.9 lock entries alongside non-Windows Inkwell 0.10 so locked builds resolve both target manifests.
