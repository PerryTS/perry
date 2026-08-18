## Restore async frame release selectivity

Compiler-private async step closures no longer register their complete boxed
activation frame as escaped closure edges. Their queued and running lifetime is
already covered by the async activation token, so terminal cells can publish
as soon as that token drains. User closures created inside the step keep the
exact per-cell capture tracking added for #8213.
