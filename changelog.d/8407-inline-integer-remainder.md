Inline `%` for runtime-proven i32 operands instead of unconditionally calling
libm `fmod`, while retaining the floating fallback for fractional, non-finite,
zero-divisor, and wide-number cases. On `pipeline`, 20-run batches improve wall
time by 6.7% and retired instructions by 1.6% with unchanged peak RSS.
