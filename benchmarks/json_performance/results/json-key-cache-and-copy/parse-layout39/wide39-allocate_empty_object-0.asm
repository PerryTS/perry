/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bc154 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object>:
1004bc154:      stp x22, x21, [sp, #-0x30]!
1004bc158:      stp x20, x19, [sp, #0x10]
1004bc15c:      stp x29, x30, [sp, #0x20]
1004bc160:      add x29, sp, #0x20
1004bc164:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc168:      add x0, x0, #0x9d0
1004bc16c:      ldr x8, [x0]
1004bc170:      blr x8
1004bc174:      mov x19, x0
1004bc178:      ldrb    w8, [x0]
1004bc17c:      strb    wzr, [x0]
1004bc180:      cbz w8, 0x1004bc1bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x68>
1004bc184:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc188:      add x0, x0, #0x9e8
1004bc18c:      ldr x8, [x0]
1004bc190:      blr x8
1004bc194:      ldrb    w8, [x0]
1004bc198:      tst w8, #0x3
1004bc19c:      b.ne    0x1004bc1b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x60>
1004bc1a0:      adrp    x8, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004bc1a4:      add x8, x8, #0x8fc
1004bc1a8:      ldapr   w8, [x8]
1004bc1ac:      cmp w8, #0x0
1004bc1b0:      b.le    0x1004bc278 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x124>
1004bc1b4:      mov w8, #0x1                ; =1
1004bc1b8:      strb    w8, [x19]
1004bc1bc:      mov w0, #0x0                ; =0
1004bc1c0:      mov w1, #0x0                ; =0
1004bc1c4:      mov w2, #0x0                ; =0
1004bc1c8:      bl  0x10081d7c0 <_js_object_alloc_with_parent>
1004bc1cc:      mov x20, x0
1004bc1d0:      cbz x0, 0x1004bc1e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x8c>
1004bc1d4:      ldurh   w8, [x20, #-0x6]
1004bc1d8:      orr w8, w8, #0x200
1004bc1dc:      sturh   w8, [x20, #-0x6]
1004bc1e0:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004bc1e4:      add x0, x0, #0xb60
1004bc1e8:      ldr x8, [x0]
1004bc1ec:      blr x8
1004bc1f0:      ldrb    w8, [x0, #0x38]
1004bc1f4:      cmp w8, #0x1
1004bc1f8:      b.ne    0x1004bc2e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x190>
1004bc1fc:      ldr x8, [x0]
1004bc200:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004bc204:      cmp x8, x9
1004bc208:      b.hs    0x1004bc2f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1a4>
1004bc20c:      ldr x8, [x0, #0x20]
1004bc210:      cmp x8, #0x1, lsl #12       ; =0x1000
1004bc214:      b.hi    0x1004bc304 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1b0>
1004bc218:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc21c:      add x8, x8, #0x610
1004bc220:      ldapr   x8, [x8]
1004bc224:      cbnz    x8, 0x1004bc318 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1c4>
1004bc228:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc22c:      ldrb    w8, [x8, #0x618]
1004bc230:      cbz w8, 0x1004bc260 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc234:      bl  0x1004e0d7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena4walk18arena_in_use_bytes>
1004bc238:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc23c:      add x8, x8, #0xdb0
1004bc240:      ldapr   x8, [x8]
1004bc244:      cbnz    x8, 0x1004bc32c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1d8>
1004bc248:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc24c:      ldr x8, [x8, #0xdb8]
1004bc250:      cmp x0, x8
1004bc254:      b.lo    0x1004bc260 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc258:      mov w8, #0x1                ; =1
1004bc25c:      strb    w8, [x19]
1004bc260:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1004bc264:      bfxil   x0, x20, #0, #48
1004bc268:      ldp x29, x30, [sp, #0x20]
1004bc26c:      ldp x20, x19, [sp, #0x10]
1004bc270:      ldp x22, x21, [sp], #0x30
1004bc274:      ret
1004bc278:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc27c:      add x0, x0, #0xda8
1004bc280:      ldr x8, [x0]
1004bc284:      blr x8
1004bc288:      ldr x8, [x0]
1004bc28c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc290:      add x0, x0, #0x808
1004bc294:      ldr x9, [x0]
1004bc298:      blr x9
1004bc29c:      ldr x9, [x0]
1004bc2a0:      cmp x9, x8
1004bc2a4:      b.ls    0x1004bc2c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x170>
1004bc2a8:      str x8, [x0]
1004bc2ac:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc2b0:      add x0, x0, #0x778
1004bc2b4:      ldr x8, [x0]
1004bc2b8:      blr x8
1004bc2bc:      mov w8, #0x1                ; =1
1004bc2c0:      strb    w8, [x0]
1004bc2c4:      bl  0x1004346c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004bc2c8:      mov w0, #0x0                ; =0
1004bc2cc:      mov w1, #0x0                ; =0
1004bc2d0:      mov w2, #0x0                ; =0
1004bc2d4:      bl  0x10081d7c0 <_js_object_alloc_with_parent>
1004bc2d8:      mov x20, x0
1004bc2dc:      cbnz    x0, 0x1004bc1d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x80>
1004bc2e0:      b   0x1004bc1e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x8c>
1004bc2e4:      bl  0x100abcdf0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004bc2e8:      cbnz    x0, 0x1004bc1fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xa8>
1004bc2ec:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004bc2f0:      add x0, x0, #0x850
1004bc2f4:      bl  0x100aef91c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004bc2f8:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004bc2fc:      add x0, x0, #0x3f8
1004bc300:      bl  0x100ab151c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004bc304:      bl  0x100ade908 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004bc308:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc30c:      add x8, x8, #0x610
1004bc310:      ldapr   x8, [x8]
1004bc314:      cbz x8, 0x1004bc228 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xd4>
1004bc318:      bl  0x100ab80cc <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs7wa3bwJW6LN_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1004bc31c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc320:      ldrb    w8, [x8, #0x618]
1004bc324:      cbnz    w8, 0x1004bc234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xe0>
1004bc328:      b   0x1004bc260 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc32c:      mov x21, x0
1004bc330:      bl  0x100ab8b84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1004bc334:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc338:      ldr x8, [x8, #0xdb8]
1004bc33c:      cmp x21, x8
1004bc340:      b.hs    0x1004bc258 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x104>
1004bc344:      b   0x1004bc260 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
