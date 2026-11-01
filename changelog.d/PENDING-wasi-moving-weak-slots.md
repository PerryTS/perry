Repair remembered weak targets during copying GC without retaining them.
Keep their generational custody across later minors, and process old weak
holders so dead young targets clear. This addresses a forced-moving WASI P3
resource test that found a stale FinalizationRegistry target.
