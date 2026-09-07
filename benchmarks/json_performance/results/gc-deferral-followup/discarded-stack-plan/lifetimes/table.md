# Supplemental lifetime checks

Three randomized fresh-process triples per row. Total CPU includes explicit final cleanup. Parent is construction batch; checkpoint is bounded GC deferral. These clock-instrumented workloads are separate from the fixed 38-row matrix.

| Fixture | Operation | Lifetime | Calls | Checkpoint vs parent CPU | Candidate vs checkpoint CPU | Candidate vs checkpoint peak MiB |
|---|---|---|---:|---:|---:|---:|
| records_object_1m | stringify | discard | 120 | +0.74% | -0.14% | +0.078 |
| records_object_20m | roundtrip | discard | 7 | -0.29% | -0.28% | +0.078 |
| records_object_20m | roundtrip | discard | 8 | +4.61% | -4.72% | +0.047 |
| records_object_20m | roundtrip | discard | 9 | -4.11% | +3.69% | -0.406 |
| records_object_20m | roundtrip | discard | 16 | -2.69% | +2.42% | +293.656 |
| records_object_20m | roundtrip | discard | 24 | -7.80% | +7.98% | +797.250 |
| small_record | stringify | discard | 100000 | +2.56% | -2.09% | +0.062 |
| small_record | stringify | latest | 100000 | +3.18% | -2.98% | +0.078 |
| small_record | stringify | retain | 100000 | +2.40% | -1.78% | +0.031 |
| wide_1m | parse | discard | 60 | -43.27% | +0.63% | +0.578 |
| wide_1m | parse | latest | 60 | -43.86% | +0.36% | -0.016 |
| wide_1m | parse | retain | 8 | -0.88% | +0.70% | -0.016 |
