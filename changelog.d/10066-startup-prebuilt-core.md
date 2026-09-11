Add a prebuilt core runtime for source-free Unix installs. The compiler selects
it only when the existing feature analysis requires its supported runtime-only
subset; optional engines, native modules, dynamic evaluation, and unavailable
core archives retain their existing fallback. The profile preserves unwinding
and the allocator policy. It is intended to reduce linked binary size; startup
and RSS effects are measured separately.
Global-object access retains the full runtime because a runtime property key can
reach optional constructors without their names appearing in source.
