Avoid allocating and copying a completed concat suffix when a string builder
appends it to the shared empty string. The optimization reuses only shared
sources, so a uniquely owned source can never gain an untracked alias with
in-place mutation permission. Codegen's tag-checked heap-string append arms use
the same identity path without repeating generic pointer validation; all other
cases retain the existing `js_string_append` behavior.

On `iso_miss`, median instructions retired fall from 14.790 G to 12.471 G
(-15.7%), recovering about 70% of #8417's measured regression while keeping
#8394's accumulator-chain fix. The #8394 fixture stays flat at 27.0 M
instructions, and its 2k/4k/8k/16k scaling probe remains 0-1 ms rather than
returning to the pre-#8417 quadratic 17/56/342/1450 ms curve. Peak RSS is flat
for both workloads, and all 19 sweep programs remain byte-exact.
