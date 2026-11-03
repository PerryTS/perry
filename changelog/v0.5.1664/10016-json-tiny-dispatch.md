Accelerate repeated tiny `JSON.parse` and `JSON.stringify` calls with bounded
native decoding, cache-carried final object births, amortized parse-boundary
pressure polling, and early stable-output reuse. Canonical one-field parse and
stringify now outperform the measured Node and Bun baselines while preserving
fresh object identity, moving-GC safety, and peak/retained memory use.
