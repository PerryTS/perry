# Fresh merged-main escaped-record validation

Fresh matched compiler/runtime-static/stdlib-static build from main
`eee3881c464bf91ae900e87a42bed072bbdfc95a` (version 0.5.1529). All 70 emitted
RESULT records have finite checksums; all 42 candidate comparisons match Node
across 14 cases and normal/scheduled/full-GC modes. Twelve comparisons fail in
the unfixed `e7223f700` negative control. See metadata.json and matrix.json.
The exact portable driver invocation used --worker .work/main-eee/worker,
--baseline-worker .work/main-e722/worker and Node 26.5.1. This is correctness
evidence, not CPU timing or proof that these one-parse cases themselves moved.
Actual moving/protected witnesses are in ../main-eee-validation.
