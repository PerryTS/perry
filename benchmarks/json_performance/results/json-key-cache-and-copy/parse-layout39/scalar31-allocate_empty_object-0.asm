/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004bbf54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object>:
1004bbf54:      stp x22, x21, [sp, #-0x30]!
1004bbf58:      stp x20, x19, [sp, #0x10]
1004bbf5c:      stp x29, x30, [sp, #0x20]
1004bbf60:      add x29, sp, #0x20
1004bbf64:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bbf68:      add x0, x0, #0x9d0
1004bbf6c:      ldr x8, [x0]
1004bbf70:      blr x8
1004bbf74:      mov x19, x0
1004bbf78:      ldrb    w8, [x0]
1004bbf7c:      strb    wzr, [x0]
1004bbf80:      cbz w8, 0x1004bbfbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x68>
1004bbf84:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bbf88:      add x0, x0, #0x9e8
1004bbf8c:      ldr x8, [x0]
1004bbf90:      blr x8
1004bbf94:      ldrb    w8, [x0]
1004bbf98:      tst w8, #0x3
1004bbf9c:      b.ne    0x1004bbfb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x60>
1004bbfa0:      adrp    x8, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004bbfa4:      add x8, x8, #0x8fc
1004bbfa8:      ldapr   w8, [x8]
1004bbfac:      cmp w8, #0x0
1004bbfb0:      b.le    0x1004bc078 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x124>
1004bbfb4:      mov w8, #0x1                ; =1
1004bbfb8:      strb    w8, [x19]
1004bbfbc:      mov w0, #0x0                ; =0
1004bbfc0:      mov w1, #0x0                ; =0
1004bbfc4:      mov w2, #0x0                ; =0
1004bbfc8:      bl  0x10081ce80 <_js_object_alloc_with_parent>
1004bbfcc:      mov x20, x0
1004bbfd0:      cbz x0, 0x1004bbfe0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x8c>
1004bbfd4:      ldurh   w8, [x20, #-0x6]
1004bbfd8:      orr w8, w8, #0x200
1004bbfdc:      sturh   w8, [x20, #-0x6]
1004bbfe0:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004bbfe4:      add x0, x0, #0xb60
1004bbfe8:      ldr x8, [x0]
1004bbfec:      blr x8
1004bbff0:      ldrb    w8, [x0, #0x38]
1004bbff4:      cmp w8, #0x1
1004bbff8:      b.ne    0x1004bc0e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x190>
1004bbffc:      ldr x8, [x0]
1004bc000:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004bc004:      cmp x8, x9
1004bc008:      b.hs    0x1004bc0f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1a4>
1004bc00c:      ldr x8, [x0, #0x20]
1004bc010:      cmp x8, #0x1, lsl #12       ; =0x1000
1004bc014:      b.hi    0x1004bc104 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1b0>
1004bc018:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc01c:      add x8, x8, #0x610
1004bc020:      ldapr   x8, [x8]
1004bc024:      cbnz    x8, 0x1004bc118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1c4>
1004bc028:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc02c:      ldrb    w8, [x8, #0x618]
1004bc030:      cbz w8, 0x1004bc060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc034:      bl  0x1004e043c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena4walk18arena_in_use_bytes>
1004bc038:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc03c:      add x8, x8, #0xdb0
1004bc040:      ldapr   x8, [x8]
1004bc044:      cbnz    x8, 0x1004bc12c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x1d8>
1004bc048:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc04c:      ldr x8, [x8, #0xdb8]
1004bc050:      cmp x0, x8
1004bc054:      b.lo    0x1004bc060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc058:      mov w8, #0x1                ; =1
1004bc05c:      strb    w8, [x19]
1004bc060:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1004bc064:      bfxil   x0, x20, #0, #48
1004bc068:      ldp x29, x30, [sp, #0x20]
1004bc06c:      ldp x20, x19, [sp, #0x10]
1004bc070:      ldp x22, x21, [sp], #0x30
1004bc074:      ret
1004bc078:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc07c:      add x0, x0, #0xda8
1004bc080:      ldr x8, [x0]
1004bc084:      blr x8
1004bc088:      ldr x8, [x0]
1004bc08c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc090:      add x0, x0, #0x808
1004bc094:      ldr x9, [x0]
1004bc098:      blr x9
1004bc09c:      ldr x9, [x0]
1004bc0a0:      cmp x9, x8
1004bc0a4:      b.ls    0x1004bc0c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x170>
1004bc0a8:      str x8, [x0]
1004bc0ac:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004bc0b0:      add x0, x0, #0x778
1004bc0b4:      ldr x8, [x0]
1004bc0b8:      blr x8
1004bc0bc:      mov w8, #0x1                ; =1
1004bc0c0:      strb    w8, [x0]
1004bc0c4:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004bc0c8:      mov w0, #0x0                ; =0
1004bc0cc:      mov w1, #0x0                ; =0
1004bc0d0:      mov w2, #0x0                ; =0
1004bc0d4:      bl  0x10081ce80 <_js_object_alloc_with_parent>
1004bc0d8:      mov x20, x0
1004bc0dc:      cbnz    x0, 0x1004bbfd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x80>
1004bc0e0:      b   0x1004bbfe0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x8c>
1004bc0e4:      bl  0x100abc4b0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004bc0e8:      cbnz    x0, 0x1004bbffc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xa8>
1004bc0ec:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004bc0f0:      add x0, x0, #0x850
1004bc0f4:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004bc0f8:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004bc0fc:      add x0, x0, #0x3c8
1004bc100:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004bc104:      bl  0x100addfc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004bc108:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc10c:      add x8, x8, #0x610
1004bc110:      ldapr   x8, [x8]
1004bc114:      cbz x8, 0x1004bc028 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xd4>
1004bc118:      bl  0x100ab778c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs7wa3bwJW6LN_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1004bc11c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc120:      ldrb    w8, [x8, #0x618]
1004bc124:      cbnz    w8, 0x1004bc034 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0xe0>
1004bc128:      b   0x1004bc060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
1004bc12c:      mov x21, x0
1004bc130:      bl  0x100ab8244 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1004bc134:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004bc138:      ldr x8, [x8, #0xdb8]
1004bc13c:      cmp x21, x8
1004bc140:      b.hs    0x1004bc058 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x104>
1004bc144:      b   0x1004bc060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json11parse_empty21allocate_empty_object+0x10c>
