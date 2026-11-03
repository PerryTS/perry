The ECS benchmark specializations now cover their real cross-module integration paths: short
packed spread and exact argument-shape calls receive producer metadata, imported object-literal
methods retain exact own-method capabilities through adapter parameters, and closure-captured
packed loops version nested arrays derived from guarded indexed reads. Every path keeps a guarded
generic side exit. The audit also fixes mixed fixed/spread `Math` calls incorrectly treating their
tail array as a scalar argument and restores iterator-protocol fallback for proxy and Array-subclass
spread tails. On the controlled M1 cohort, the corrected full Wolf workload is 1.73x faster and the
imported-method `perform-ecs` workload is 11.05% faster; the short-spread and argument-clone slices
activate correctly but remain performance-neutral.
