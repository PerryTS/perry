### perf(gc): stamp exact starts only for Maps

Arena allocation no longer updates the object-start bitmap for every Object
and Array. Only Maps need allocation-authored boundary evidence: their type tag
is 8-aligned and their copying descriptor follows an external entries pointer.
The classifier retains that exact-start check for Maps and rejects malloc-only
descriptors before arena dispatch.

On the clean 19-row M1 mini sweep, allocation-class instructions improve from
**+3.69% to -0.01%** versus the pre-bitmap baseline; the prior -48.3% cohort
remains improved at **-48.90%**. Peak Perry RSS is 404,520,960 bytes versus
421,429,248 bytes at baseline. Fixes #8288.
