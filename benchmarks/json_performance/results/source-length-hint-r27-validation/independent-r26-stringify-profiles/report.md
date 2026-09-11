# R26 stringify gap profiles

Frozen measured R26 source `3aac4d6335da54abeeed73df842decbbe6dd5d71`, normal production worker. Three bounded sampling diagnostics, quiet host, all terminal windows archived first. Full output and complete loop checksums match Node 26.5.1. Sampled CPU/RSS is diagnostic only and is not uninstrumented benchmark evidence.

## small_record / pretty

684 workload samples. Reported inclusive phases overlap and must not be added. Zero named-root samples do not establish that inlined root work is absent.

- memory_copy: 43 inclusive samples (6.3%).
- allocation: 27 inclusive samples (3.9%).
- collection: 13 inclusive samples (1.9%).
- root_scope: 0 inclusive samples (0.0%).

Largest self-sample nodes (the same function may occur under multiple parents):

- 62: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 432,420,...  [0x100fddff4,0x100fddfe8,...]`
- 55: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer23stringify_object_pretty  (in r26-options-worker) + 848,1856,...  [0x100feb490,0x100feb880,...]`
- 35: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer22stringify_value_pretty  (in r26-options-worker) + 100,1744,...  [0x100fea644,0x100feacb0,...]`
- 23: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 420,1384,...  [0x100fddfe8,0x100fde3ac,...]`
- 22: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 40,448,...  [0x100fdde6c,0x100fde004,...]`
- 19: `js_json_stringify_full  (in r26-options-worker) + 3472,4008,...  [0x10132fcd0,0x10132fee8,...]`

## small_record / callback

742 workload samples. Reported inclusive phases overlap and must not be added. Zero named-root samples do not establish that inlined root work is absent.

- memory_copy: 30 inclusive samples (4.0%).
- allocation: 60 inclusive samples (8.1%).
- collection: 3 inclusive samples (0.4%).
- root_scope: 69 inclusive samples (9.3%).

Largest self-sample nodes (the same function may occur under multiple parents):

- 58: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer37stringify_object_with_replacer_pretty  (in r26-options-worker) + 712,1688,...  [0x1047d6988,0x1047d6d58,...]`
- 34: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer13call_replacer  (in r26-options-worker) + 68,132,...  [0x1047d0dd8,0x1047d0e18,...]`
- 30: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 448,432,...  [0x1047c6004,0x1047c5ff4,...]`
- 22: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer19apply_to_json_keyed  (in r26-options-worker) + 452,40,...  [0x1047d1140,0x1047d0fa4,...]`
- 22: `js_closure_call2  (in r26-options-worker) + 160,132,...  [0x104ad7f20,0x104ad7f04,...]`
- 15: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 432,440,...  [0x1047c5ff4,0x1047c5ffc,...]`

## records_array_16k / pretty

730 workload samples. Reported inclusive phases overlap and must not be added. Zero named-root samples do not establish that inlined root work is absent.

- memory_copy: 92 inclusive samples (12.6%).
- allocation: 41 inclusive samples (5.6%).
- collection: 28 inclusive samples (3.8%).
- root_scope: 0 inclusive samples (0.0%).

Largest self-sample nodes (the same function may occur under multiple parents):

- 81: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer23stringify_object_pretty  (in r26-options-worker) + 1316,884,...  [0x102edf664,0x102edf4b4,...]`
- 67: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 432,172,...  [0x102ed1ff4,0x102ed1ef0,...]`
- 42: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer22stringify_value_pretty  (in r26-options-worker) + 100,132,...  [0x102ede644,0x102ede664,...]`
- 31: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json17stringify_scalars20write_escaped_string  (in r26-options-worker) + 1388,448,...  [0x102ed23b0,0x102ed2004,...]`
- 29: `_RNvNtNtCs3gRpqoBkMHm_13perry_runtime4json8replacer22stringify_array_pretty  (in r26-options-worker) + 1364,1268,...  [0x102ede314,0x102ede2b4,...]`
- 27: `_platform_memmove  (in libsystem_platform.dylib) + 452,12,...  [0x18171f524,0x18171f36c,...]`
