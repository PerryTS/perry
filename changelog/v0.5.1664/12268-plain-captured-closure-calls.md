Compiled closures without rest or arguments bundling may tail-call their body from the per-arity runtime entry. Short calls, bound values, native bodies, and rest bodies retain dispatch. The eligibility word preserves zero as the legacy padding value, so older provider records never enter the direct path; adding a rest kind revokes eligibility regardless of builder order.

Captured immutable closures with erased function types call their proven same-module bodies. Reassigned bindings and under-applied calls retain dynamic dispatch and argument padding.

Wide dynamic calls use the body's declared parameter width, dropping surplus arguments instead of widening a short body past the dynamic-call ABI limit.
