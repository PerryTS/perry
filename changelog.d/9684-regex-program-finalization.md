**Compiled RegExp programs no longer remain allocated after their JavaScript
owner is collected.** RegExp headers held raw `Arc` references for the standard,
fancy, and RepeatMatcher programs, but their GC finalizer cleared only metadata
side tables. The finalizer now releases all three owned references while
preserving ownership across live arena moves, preventing the permanent
per-pattern memory growth reported in #9678.
