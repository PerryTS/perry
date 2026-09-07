/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100612ac0 <_js_array_grow>:
100612ac0:      sub sp, sp, #0xb0
100612ac4:      stp x28, x27, [sp, #0x50]
100612ac8:      stp x26, x25, [sp, #0x60]
100612acc:      stp x24, x23, [sp, #0x70]
100612ad0:      stp x22, x21, [sp, #0x80]
100612ad4:      stp x20, x19, [sp, #0x90]
100612ad8:      stp x29, x30, [sp, #0xa0]
100612adc:      add x29, sp, #0xa0
100612ae0:      cmp x0, #0xfff
100612ae4:      b.ls    0x100612e34 <_js_array_grow+0x374>
100612ae8:      mov x19, x0
100612aec:      lsr x8, x0, #51
100612af0:      cmp x8, #0xfff
100612af4:      b.lo    0x100612b0c <_js_array_grow+0x4c>
100612af8:      mov w8, #0x7ffc             ; =32764
100612afc:      cmp x8, x19, lsr #48
100612b00:      b.eq    0x100612e34 <_js_array_grow+0x374>
100612b04:      ands    x19, x19, #0xffffffffffff
100612b08:      b.eq    0x100612e34 <_js_array_grow+0x374>
100612b0c:      and x8, x19, #0xfffffffffff00000
100612b10:      lsr x9, x19, #47
100612b14:      cmp x9, #0x0
100612b18:      ccmp    x8, #0x0, #0x4, eq
100612b1c:      b.eq    0x100612e34 <_js_array_grow+0x374>
100612b20:      tst x19, #0x3
100612b24:      ccmp    x19, #0x7, #0x0, eq
100612b28:      mov x22, x1
100612b2c:      b.ls    0x100612c34 <_js_array_grow+0x174>
100612b30:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
100612b34:      add x8, x8, #0x360
100612b38:      ldr x8, [x8]
100612b3c:      cmn x8, #0x1
100612b40:      b.eq    0x100613018 <_js_array_grow+0x558>
100612b44:      mrs x9, TPIDRRO_EL0
100612b48:      and x9, x9, #0xfffffffffffffff8
100612b4c:      ldr x0, [x9, x8, lsl #3]
100612b50:      cbz x0, 0x100613018 <_js_array_grow+0x558>
100612b54:      lsr x1, x19, #20
100612b58:      ldr x8, [x0, #0x10]
100612b5c:      ldrb    w9, [x8, #0x28]
100612b60:      tbz w9, #0x0, 0x100612b80 <_js_array_grow+0xc0>
100612b64:      ldr x9, [x8, #0x20]
100612b68:      cmp x9, x1
100612b6c:      b.ne    0x100612b80 <_js_array_grow+0xc0>
100612b70:      ldp x9, x10, [x8]
100612b74:      cmp x9, x19
100612b78:      ccmp    x10, x19, #0x0, ls
100612b7c:      b.hi    0x100612bfc <_js_array_grow+0x13c>
100612b80:      ldrb    w9, [x8, #0x58]
100612b84:      cbz w9, 0x100612ba4 <_js_array_grow+0xe4>
100612b88:      ldr x9, [x8, #0x50]
100612b8c:      cmp x9, x1
100612b90:      b.ne    0x100612ba4 <_js_array_grow+0xe4>
100612b94:      ldp x9, x10, [x8, #0x30]
100612b98:      cmp x9, x19
100612b9c:      ccmp    x10, x19, #0x0, ls
100612ba0:      b.hi    0x100612bf0 <_js_array_grow+0x130>
100612ba4:      ldrb    w9, [x8, #0x88]
100612ba8:      cbz w9, 0x100612bc8 <_js_array_grow+0x108>
100612bac:      ldr x9, [x8, #0x80]
100612bb0:      cmp x9, x1
100612bb4:      b.ne    0x100612bc8 <_js_array_grow+0x108>
100612bb8:      ldp x9, x10, [x8, #0x60]
100612bbc:      cmp x9, x19
100612bc0:      ccmp    x10, x19, #0x0, ls
100612bc4:      b.hi    0x100612bf8 <_js_array_grow+0x138>
100612bc8:      ldrb    w9, [x8, #0xb8]
100612bcc:      cbz w9, 0x100612c08 <_js_array_grow+0x148>
100612bd0:      ldr x9, [x8, #0xb0]
100612bd4:      cmp x9, x1
100612bd8:      b.ne    0x100612c08 <_js_array_grow+0x148>
100612bdc:      ldp x9, x10, [x8, #0x90]!
100612be0:      cmp x9, x19
100612be4:      ccmp    x10, x19, #0x0, ls
100612be8:      b.hi    0x100612bfc <_js_array_grow+0x13c>
100612bec:      b   0x100612c08 <_js_array_grow+0x148>
100612bf0:      add x8, x8, #0x30
100612bf4:      b   0x100612bfc <_js_array_grow+0x13c>
100612bf8:      add x8, x8, #0x60
100612bfc:      ldrb    w8, [x8, #0x19]
100612c00:      cmp w8, #0xff
100612c04:      b.ne    0x100612c14 <_js_array_grow+0x154>
100612c08:      mov x0, x19
100612c0c:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100612c10:      and w8, w0, #0xff
100612c14:      cbz w8, 0x100612c34 <_js_array_grow+0x174>
100612c18:      ldurb   w8, [x19, #-0x8]
100612c1c:      ldurb   w9, [x19, #-0x7]
100612c20:      mov w10, #0x82              ; =130
100612c24:      and w9, w9, w10
100612c28:      cmp w9, #0x2
100612c2c:      ccmp    w8, #0x1, #0x0, eq
100612c30:      b.eq    0x100612e58 <_js_array_grow+0x398>
100612c34:      mov x0, x19
100612c38:      bl  0x1005cf8e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100612c3c:      mov x8, x0
100612c40:      cbz x0, 0x100612cdc <_js_array_grow+0x21c>
100612c44:      ldrb    w9, [x8]
100612c48:      cmp w9, #0x1
100612c4c:      b.ne    0x100612d6c <_js_array_grow+0x2ac>
100612c50:      ldrsb   w9, [x8, #0x1]
100612c54:      mov x0, x8
100612c58:      tbz w9, #0x1f, 0x100612db0 <_js_array_grow+0x2f0>
100612c5c:      mov x20, x8
100612c60:      ldr x19, [x8, #0x8]
100612c64:      mov x0, x19
100612c68:      bl  0x1005cf8e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100612c6c:      mov x1, x22
100612c70:      cbz x0, 0x100612e34 <_js_array_grow+0x374>
100612c74:      ldrb    w8, [x0]
100612c78:      cmp w8, #0x1
100612c7c:      b.ne    0x100612e34 <_js_array_grow+0x374>
100612c80:      ldrsb   w8, [x0, #0x1]
100612c84:      tbz w8, #0x1f, 0x100612d64 <_js_array_grow+0x2a4>
100612c88:      mov w21, #0x1               ; =1
100612c8c:      ldr x19, [x0, #0x8]
100612c90:      mov x0, x19
100612c94:      bl  0x1005cf8e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100612c98:      mov x1, x22
100612c9c:      cbz x0, 0x100612e34 <_js_array_grow+0x374>
100612ca0:      ldrb    w8, [x0]
100612ca4:      cmp w8, #0x1
100612ca8:      b.ne    0x100612e34 <_js_array_grow+0x374>
100612cac:      cmp w21, #0x3f
100612cb0:      b.hi    0x100612e34 <_js_array_grow+0x374>
100612cb4:      add w21, w21, #0x1
100612cb8:      ldrsb   w8, [x0, #0x1]
100612cbc:      tbnz    w8, #0x1f, 0x100612c8c <_js_array_grow+0x1cc>
100612cc0:      mov x8, x20
100612cc4:      str x19, [x20, #0x8]
100612cc8:      ldrb    w9, [x20, #0x1]
100612ccc:      orr w9, w9, #0x80
100612cd0:      strb    w9, [x20, #0x1]
100612cd4:      ldrb    w9, [x0]
100612cd8:      b   0x100612d74 <_js_array_grow+0x2b4>
100612cdc:      mov x20, x8
100612ce0:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100612ce4:      add x8, x8, #0xb34
100612ce8:      ldaprb  w8, [x8]
100612cec:      cbz w8, 0x100612d1c <_js_array_grow+0x25c>
100612cf0:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100612cf4:      add x8, x8, #0xef8
100612cf8:      ldapr   x9, [x8]
100612cfc:      cmp x9, x19
100612d00:      b.hi    0x100612d1c <_js_array_grow+0x25c>
100612d04:      ldapur  x8, [x8, #0x8]
100612d08:      cmp x8, x19
100612d0c:      b.lo    0x100612d1c <_js_array_grow+0x25c>
100612d10:      mov x0, x19
100612d14:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100612d18:      tbnz    w0, #0x0, 0x100612d60 <_js_array_grow+0x2a0>
100612d1c:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100612d20:      add x8, x8, #0x620
100612d24:      ldaprb  w8, [x8]
100612d28:      mov x1, x22
100612d2c:      cbz w8, 0x100612e34 <_js_array_grow+0x374>
100612d30:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100612d34:      add x8, x8, #0xc08
100612d38:      ldapr   x9, [x8]
100612d3c:      cmp x9, x19
100612d40:      b.hi    0x100612e34 <_js_array_grow+0x374>
100612d44:      ldapur  x8, [x8, #0x8]
100612d48:      cmp x8, x19
100612d4c:      b.lo    0x100612e34 <_js_array_grow+0x374>
100612d50:      mov x0, x19
100612d54:      bl  0x100948dac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100612d58:      mov x1, x22
100612d5c:      tbz w0, #0x0, 0x100612e34 <_js_array_grow+0x374>
100612d60:      mov x0, #0x0                ; =0
100612d64:      mov x8, x20
100612d68:      b   0x100612db0 <_js_array_grow+0x2f0>
100612d6c:      mov x0, x8
100612d70:      mov x1, x22
100612d74:      cmp w9, #0x1
100612d78:      b.eq    0x100612db0 <_js_array_grow+0x2f0>
100612d7c:      cmp w9, #0x9
100612d80:      b.ne    0x100612e34 <_js_array_grow+0x374>
100612d84:      ldr w8, [x19, #0x4]
100612d88:      mov w9, #0x5841             ; =22593
100612d8c:      movk    w9, #0x4c5a, lsl #16
100612d90:      cmp w8, w9
100612d94:      b.ne    0x100612e34 <_js_array_grow+0x374>
100612d98:      mov x0, x19
100612d9c:      bl  0x1008ae598 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
100612da0:      mov x1, x22
100612da4:      cbz x0, 0x100612e34 <_js_array_grow+0x374>
100612da8:      mov x19, x0
100612dac:      b   0x100612e7c <_js_array_grow+0x3bc>
100612db0:      ldp w10, w9, [x19]
100612db4:      cmp w10, w9
100612db8:      b.ls    0x100612dd8 <_js_array_grow+0x318>
100612dbc:      cbz x8, 0x100612de8 <_js_array_grow+0x328>
100612dc0:      ldr w8, [x0, #0x4]
100612dc4:      lsl x9, x9, #3
100612dc8:      add x9, x9, #0x10
100612dcc:      cmp x9, x8
100612dd0:      b.ne    0x100612de8 <_js_array_grow+0x328>
100612dd4:      b   0x100612e7c <_js_array_grow+0x3bc>
100612dd8:      mov w8, #0xe100             ; =57600
100612ddc:      movk    w8, #0x5f5, lsl #16
100612de0:      cmp w10, w8
100612de4:      b.ls    0x100612e7c <_js_array_grow+0x3bc>
100612de8:      adrp    x8, 0x10120d000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfe9c>
100612dec:      add x8, x8, #0xb34
100612df0:      ldaprb  w8, [x8]
100612df4:      cbz w8, 0x100612e24 <_js_array_grow+0x364>
100612df8:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100612dfc:      add x8, x8, #0xef8
100612e00:      ldapr   x9, [x8]
100612e04:      cmp x9, x19
100612e08:      b.hi    0x100612e24 <_js_array_grow+0x364>
100612e0c:      ldapur  x8, [x8, #0x8]
100612e10:      cmp x8, x19
100612e14:      b.lo    0x100612e24 <_js_array_grow+0x364>
100612e18:      mov x0, x19
100612e1c:      bl  0x100a15a18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100612e20:      tbnz    w0, #0x0, 0x100612e7c <_js_array_grow+0x3bc>
100612e24:      mov x0, x19
100612e28:      bl  0x1005a8970 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
100612e2c:      mov x1, x22
100612e30:      tbnz    w0, #0x0, 0x100612e7c <_js_array_grow+0x3bc>
100612e34:      mov x0, x1
100612e38:      ldp x29, x30, [sp, #0xa0]
100612e3c:      ldp x20, x19, [sp, #0x90]
100612e40:      ldp x22, x21, [sp, #0x80]
100612e44:      ldp x24, x23, [sp, #0x70]
100612e48:      ldp x26, x25, [sp, #0x60]
100612e4c:      ldp x28, x27, [sp, #0x50]
100612e50:      add sp, sp, #0xb0
100612e54:      b   0x1008f7cc0 <_js_array_alloc>
100612e58:      ldr w8, [x19]
100612e5c:      mov w9, #0xe100             ; =57600
100612e60:      movk    w9, #0x5f5, lsl #16
100612e64:      orr w9, w9, #0x1
100612e68:      cmp w8, w9
100612e6c:      b.hs    0x100612c34 <_js_array_grow+0x174>
100612e70:      ldr w9, [x19, #0x4]
100612e74:      cmp w8, w9
100612e78:      b.hi    0x100612c34 <_js_array_grow+0x174>
100612e7c:      mov x0, x19
100612e80:      bl  0x1005c0a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100612e84:      tst w0, #0x6
100612e88:      b.ne    0x100613680 <_js_array_grow+0xbc0>
100612e8c:      mov x0, x19
100612e90:      bl  0x1005c0a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100612e94:      tbnz    w0, #0x0, 0x100613680 <_js_array_grow+0xbc0>
100612e98:      adrp    x26, 0x101130000 <_perry_global_baseline_worker_ts__1>
100612e9c:      add x26, x26, #0x360
100612ea0:      ldr x8, [x26]
100612ea4:      cmn x8, #0x1
100612ea8:      b.eq    0x100612edc <_js_array_grow+0x41c>
100612eac:      mrs x9, TPIDRRO_EL0
100612eb0:      and x9, x9, #0xfffffffffffffff8
100612eb4:      ldr x8, [x9, x8, lsl #3]
100612eb8:      cbz x8, 0x100612edc <_js_array_grow+0x41c>
100612ebc:      ldr x8, [x8, #0x19e8]
100612ec0:      cbz x8, 0x100612edc <_js_array_grow+0x41c>
100612ec4:      ldr x9, [x8]
100612ec8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100612ecc:      cmp x9, x10
100612ed0:      b.hs    0x10061385c <_js_array_grow+0xd9c>
100612ed4:      ldr x20, [x8, #0x18]
100612ed8:      b   0x100612eec <_js_array_grow+0x42c>
100612edc:      adrp    x0, 0x1010b6000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
100612ee0:      add x0, x0, #0xd48
100612ee4:      bl  0x1001358ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100612ee8:      mov x20, x0
100612eec:      str x20, [sp, #0x20]
100612ef0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100612ef4:      stp x19, x8, [sp, #0x40]
100612ef8:      mov w8, #0x1                ; =1
100612efc:      str x8, [sp, #0x38]
100612f00:      add x0, sp, #0x38
100612f04:      bl  0x1005a84c8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100612f08:      str x0, [sp, #0x28]
100612f0c:      ldr w23, [x19, #0x4]
100612f10:      cmp w22, w23
100612f14:      b.ls    0x100612fd4 <_js_array_grow+0x514>
100612f18:      mov x21, x0
100612f1c:      lsl w8, w23, #1
100612f20:      cmp w22, w8
100612f24:      csel    w24, w22, w8, hi
100612f28:      ubfiz   x22, x24, #3, #32
100612f2c:      ldurb   w8, [x19, #-0x7]
100612f30:      tbnz    w8, #0x5, 0x1006131f0 <_js_array_grow+0x730>
100612f34:      ldr x8, [x26]
100612f38:      cmn x8, #0x1
100612f3c:      b.eq    0x1006131c8 <_js_array_grow+0x708>
100612f40:      mrs x9, TPIDRRO_EL0
100612f44:      and x9, x9, #0xfffffffffffffff8
100612f48:      ldr x0, [x9, x8, lsl #3]
100612f4c:      cbz x0, 0x1006131c8 <_js_array_grow+0x708>
100612f50:      lsr x1, x19, #20
100612f54:      ldr x8, [x0, #0x10]
100612f58:      ldrb    w9, [x8, #0x28]
100612f5c:      tbz w9, #0x0, 0x100612f7c <_js_array_grow+0x4bc>
100612f60:      ldr x9, [x8, #0x20]
100612f64:      cmp x9, x1
100612f68:      b.ne    0x100612f7c <_js_array_grow+0x4bc>
100612f6c:      ldp x9, x10, [x8]
100612f70:      cmp x9, x19
100612f74:      ccmp    x10, x19, #0x0, ls
100612f78:      b.hi    0x100613068 <_js_array_grow+0x5a8>
100612f7c:      ldrb    w9, [x8, #0x58]
100612f80:      cbz w9, 0x100612fa0 <_js_array_grow+0x4e0>
100612f84:      ldr x9, [x8, #0x50]
100612f88:      cmp x9, x1
100612f8c:      b.ne    0x100612fa0 <_js_array_grow+0x4e0>
100612f90:      ldp x9, x10, [x8, #0x30]
100612f94:      cmp x9, x19
100612f98:      ccmp    x10, x19, #0x0, ls
100612f9c:      b.hi    0x100613064 <_js_array_grow+0x5a4>
100612fa0:      ldrb    w9, [x8, #0x88]
100612fa4:      cbz w9, 0x100613030 <_js_array_grow+0x570>
100612fa8:      ldr x9, [x8, #0x80]
100612fac:      cmp x9, x1
100612fb0:      b.ne    0x100613030 <_js_array_grow+0x570>
100612fb4:      ldr x9, [x8, #0x60]
100612fb8:      cmp x9, x19
100612fbc:      b.hi    0x100613030 <_js_array_grow+0x570>
100612fc0:      ldr x9, [x8, #0x68]
100612fc4:      cmp x9, x19
100612fc8:      b.ls    0x100613030 <_js_array_grow+0x570>
100612fcc:      add x8, x8, #0x60
100612fd0:      b   0x100613068 <_js_array_grow+0x5a8>
100612fd4:      ldr x8, [x26]
100612fd8:      cmn x8, #0x1
100612fdc:      b.eq    0x100613670 <_js_array_grow+0xbb0>
100612fe0:      mrs x9, TPIDRRO_EL0
100612fe4:      and x9, x9, #0xfffffffffffffff8
100612fe8:      ldr x8, [x9, x8, lsl #3]
100612fec:      cbz x8, 0x100613670 <_js_array_grow+0xbb0>
100612ff0:      ldr x8, [x8, #0x19e8]
100612ff4:      cbz x8, 0x100613670 <_js_array_grow+0xbb0>
100612ff8:      ldr x9, [x8]
100612ffc:      cbnz    x9, 0x100613664 <_js_array_grow+0xba4>
100613000:      ldr x9, [x8, #0x18]
100613004:      cmp x20, x9
100613008:      b.hi    0x100613010 <_js_array_grow+0x550>
10061300c:      str x20, [x8, #0x18]
100613010:      str xzr, [x8]
100613014:      b   0x100613680 <_js_array_grow+0xbc0>
100613018:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10061301c:      lsr x1, x19, #20
100613020:      ldr x8, [x0, #0x10]
100613024:      ldrb    w9, [x8, #0x28]
100613028:      tbnz    w9, #0x0, 0x100612b64 <_js_array_grow+0xa4>
10061302c:      b   0x100612b80 <_js_array_grow+0xc0>
100613030:      ldrb    w9, [x8, #0xb8]
100613034:      cbz w9, 0x100613074 <_js_array_grow+0x5b4>
100613038:      ldr x9, [x8, #0xb0]
10061303c:      cmp x9, x1
100613040:      b.ne    0x100613074 <_js_array_grow+0x5b4>
100613044:      ldr x9, [x8, #0x90]
100613048:      cmp x9, x19
10061304c:      b.hi    0x100613074 <_js_array_grow+0x5b4>
100613050:      ldr x9, [x8, #0x98]
100613054:      cmp x9, x19
100613058:      b.ls    0x100613074 <_js_array_grow+0x5b4>
10061305c:      add x8, x8, #0x90
100613060:      b   0x100613068 <_js_array_grow+0x5a8>
100613064:      add x8, x8, #0x30
100613068:      ldrb    w8, [x8, #0x19]
10061306c:      cmp w8, #0xff
100613070:      b.ne    0x100613080 <_js_array_grow+0x5c0>
100613074:      mov x0, x19
100613078:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
10061307c:      and w8, w0, #0xff
100613080:      cmp w8, #0x1
100613084:      b.ne    0x1006131f0 <_js_array_grow+0x730>
100613088:      mov w8, #0x3ffe             ; =16382
10061308c:      cmp w24, w8
100613090:      b.hi    0x1006131f0 <_js_array_grow+0x730>
100613094:      ldr x8, [x26]
100613098:      cmn x8, #0x1
10061309c:      b.eq    0x1006131e0 <_js_array_grow+0x720>
1006130a0:      mrs x9, TPIDRRO_EL0
1006130a4:      and x9, x9, #0xfffffffffffffff8
1006130a8:      ldr x0, [x9, x8, lsl #3]
1006130ac:      cbz x0, 0x1006131e0 <_js_array_grow+0x720>
1006130b0:      ldr x8, [x0, #0x28]
1006130b4:      ldrb    w8, [x8]
1006130b8:      tbnz    w8, #0x0, 0x1006131f0 <_js_array_grow+0x730>
1006130bc:      ldr x8, [x26]
1006130c0:      cmn x8, #0x1
1006130c4:      b.eq    0x1006133cc <_js_array_grow+0x90c>
1006130c8:      mrs x9, TPIDRRO_EL0
1006130cc:      and x9, x9, #0xfffffffffffffff8
1006130d0:      ldr x0, [x9, x8, lsl #3]
1006130d4:      cbz x0, 0x1006133cc <_js_array_grow+0x90c>
1006130d8:      ldr x25, [x0, #0x8]
1006130dc:      ldr x8, [x26]
1006130e0:      cmn x8, #0x1
1006130e4:      b.eq    0x1006133e0 <_js_array_grow+0x920>
1006130e8:      mrs x9, TPIDRRO_EL0
1006130ec:      and x9, x9, #0xfffffffffffffff8
1006130f0:      ldr x0, [x9, x8, lsl #3]
1006130f4:      cbz x0, 0x1006133e0 <_js_array_grow+0x920>
1006130f8:      ldr x19, [x0]
1006130fc:      ldr x8, [x25]
100613100:      cbz x8, 0x100613124 <_js_array_grow+0x664>
100613104:      ldp x1, x0, [x19, #0x10]
100613108:      cmp x0, x1
10061310c:      b.hs    0x1006138c0 <_js_array_grow+0xe00>
100613110:      ldr x8, [x19, #0x8]
100613114:      ldr x9, [x25, #0x8]
100613118:      mov w10, #0x30              ; =48
10061311c:      madd    x8, x0, x10, x8
100613120:      str x9, [x8, #0x20]
100613124:      add x27, x22, #0x10
100613128:      ldr x1, [x19, #0x18]
10061312c:      add x2, x22, #0x10
100613130:      mov x0, x19
100613134:      bl  0x1005a4510 <__RNvMs1_NtNtCs5gMwpk3Cs4e_13perry_runtime5arena5blockNtB5_5Arena15try_block_alloc>
100613138:      cmp x0, #0x1
10061313c:      b.ne    0x1006131f0 <_js_array_grow+0x730>
100613140:      ldr x8, [x25]
100613144:      cbz x8, 0x100613174 <_js_array_grow+0x6b4>
100613148:      ldp x8, x0, [x19, #0x10]
10061314c:      cmp x0, x8
100613150:      b.hs    0x1006138cc <_js_array_grow+0xe0c>
100613154:      ldr x8, [x19, #0x8]
100613158:      mov w9, #0x30               ; =48
10061315c:      madd    x8, x0, x9, x8
100613160:      ldr x9, [x8, #0x10]
100613164:      ldur    q0, [x8, #0x18]
100613168:      str x9, [x25]
10061316c:      ext.16b v0, v0, v0, #0x8
100613170:      stur    q0, [x25, #0x8]
100613174:      cbz x1, 0x1006131f0 <_js_array_grow+0x730>
100613178:      mov w8, #0x1                ; =1
10061317c:      strb    w8, [x1]
100613180:      ldr x8, [x26]
100613184:      cmn x8, #0x1
100613188:      b.eq    0x10061384c <_js_array_grow+0xd8c>
10061318c:      mrs x9, TPIDRRO_EL0
100613190:      and x9, x9, #0xfffffffffffffff8
100613194:      ldr x0, [x9, x8, lsl #3]
100613198:      cbz x0, 0x10061384c <_js_array_grow+0xd8c>
10061319c:      ldr x8, [x0, #0x30]
1006131a0:      ldrb    w8, [x8]
1006131a4:      orr w8, w8, #0x2
1006131a8:      strb    w8, [x1, #0x1]
1006131ac:      mov x0, x1
1006131b0:      mov x19, x1
1006131b4:      bl  0x1005b88d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier19gc_note_black_birth>
1006131b8:      strh    wzr, [x19, #0x2]
1006131bc:      str w27, [x19, #0x4]
1006131c0:      add x19, x19, #0x8
1006131c4:      b   0x100613210 <_js_array_grow+0x750>
1006131c8:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006131cc:      lsr x1, x19, #20
1006131d0:      ldr x8, [x0, #0x10]
1006131d4:      ldrb    w9, [x8, #0x28]
1006131d8:      tbnz    w9, #0x0, 0x100612f60 <_js_array_grow+0x4a0>
1006131dc:      b   0x100612f7c <_js_array_grow+0x4bc>
1006131e0:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006131e4:      ldr x8, [x0, #0x28]
1006131e8:      ldrb    w8, [x8]
1006131ec:      tbz w8, #0x0, 0x1006130bc <_js_array_grow+0x5fc>
1006131f0:      add x0, x22, #0x8
1006131f4:      mov w1, #0x8                ; =8
1006131f8:      mov w2, #0x1                ; =1
1006131fc:      bl  0x1005bb0c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena10allocators18arena_alloc_gc_old>
100613200:      mov x19, x0
100613204:      ldurb   w8, [x0, #-0x7]
100613208:      orr w8, w8, #0x20
10061320c:      sturb   w8, [x0, #-0x7]
100613210:      ldr x8, [x26]
100613214:      cmn x8, #0x1
100613218:      b.eq    0x10061327c <_js_array_grow+0x7bc>
10061321c:      mrs x9, TPIDRRO_EL0
100613220:      and x9, x9, #0xfffffffffffffff8
100613224:      ldr x8, [x9, x8, lsl #3]
100613228:      cbz x8, 0x10061327c <_js_array_grow+0x7bc>
10061322c:      ldr x8, [x8, #0x19e8]
100613230:      cbz x8, 0x10061327c <_js_array_grow+0x7bc>
100613234:      ldr x9, [x8]
100613238:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10061323c:      cmp x9, x10
100613240:      b.hs    0x100613884 <_js_array_grow+0xdc4>
100613244:      add x10, x9, #0x1
100613248:      str x10, [x8]
10061324c:      ldr x10, [x8, #0x18]
100613250:      cmp x21, x10
100613254:      b.hs    0x100613890 <_js_array_grow+0xdd0>
100613258:      ldr x10, [x8, #0x10]
10061325c:      mov w11, #0x18              ; =24
100613260:      madd    x10, x21, x11, x10
100613264:      ldr x11, [x10]
100613268:      cmp x11, #0x1
10061326c:      b.ne    0x100613894 <_js_array_grow+0xdd4>
100613270:      ldr x21, [x10, #0x8]
100613274:      str x9, [x8]
100613278:      b   0x100613290 <_js_array_grow+0x7d0>
10061327c:      adrp    x0, 0x1010b6000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
100613280:      add x0, x0, #0xd48
100613284:      add x1, sp, #0x28
100613288:      bl  0x100135710 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10061328c:      mov x21, x0
100613290:      lsl x8, x23, #3
100613294:      add x22, x8, #0x8
100613298:      mov x0, x19
10061329c:      mov x1, x21
1006132a0:      mov x2, x22
1006132a4:      bl  0x100ce596c <_writev+0x100ce596c>
1006132a8:      str w24, [x19, #0x4]
1006132ac:      sub x8, x24, x23
1006132b0:      lsl x2, x8, #3
1006132b4:      adrp    x1, 0x100dd7000 <_anon.6ab1a69beca0dab646726cb1e935b5ae.15+0x18>
1006132b8:      add x1, x1, #0xee0
1006132bc:      add x0, x19, x22
1006132c0:      bl  0x100ce5990 <_writev+0x100ce5990>
1006132c4:      ldurh   w8, [x21, #-0x6]
1006132c8:      sturh   w8, [x19, #-0x6]
1006132cc:      mov x0, x21
1006132d0:      mov x1, x19
1006132d4:      bl  0x100318f5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout15layout_transfer>
1006132d8:      mov x0, x21
1006132dc:      mov x1, x19
1006132e0:      bl  0x1008d426c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35transfer_array_named_property_owner>
1006132e4:      mov x0, x21
1006132e8:      mov x1, x19
1006132ec:      bl  0x1001ef954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object16descriptor_state25transfer_descriptor_owner>
1006132f0:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
1006132f4:      ldr w8, [x8, #0x9c4]
1006132f8:      cbz w8, 0x100613308 <_js_array_grow+0x848>
1006132fc:      mov x0, x19
100613300:      bl  0x1005bd580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array15header_gc_slots34replay_array_growth_write_barriers>
100613304:      b   0x100613600 <_js_array_grow+0xb40>
100613308:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10061330c:      add x8, x8, #0x58
100613310:      ldapr   x8, [x8]
100613314:      cbnz    x8, 0x100613868 <_js_array_grow+0xda8>
100613318:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10061331c:      ldrb    w8, [x8, #0x60]
100613320:      cbz w8, 0x100613600 <_js_array_grow+0xb40>
100613324:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7fb58>
100613328:      ldrb    w8, [x8, #0x778]
10061332c:      cbz w8, 0x100613394 <_js_array_grow+0x8d4>
100613330:      tst x19, #0xffff800000000007
100613334:      mov w8, #0x100000           ; =1048576
100613338:      ccmp    x19, x8, #0x0, eq
10061333c:      b.lo    0x100613600 <_js_array_grow+0xb40>
100613340:      ldr x8, [x26]
100613344:      cmn x8, #0x1
100613348:      b.eq    0x1006133f4 <_js_array_grow+0x934>
10061334c:      mrs x9, TPIDRRO_EL0
100613350:      and x9, x9, #0xfffffffffffffff8
100613354:      ldr x0, [x9, x8, lsl #3]
100613358:      cbz x0, 0x1006133f4 <_js_array_grow+0x934>
10061335c:      lsr x1, x19, #20
100613360:      ldr x8, [x0, #0x10]
100613364:      ldrb    w9, [x8, #0x28]
100613368:      tbz w9, #0x0, 0x100613408 <_js_array_grow+0x948>
10061336c:      ldr x9, [x8, #0x20]
100613370:      cmp x9, x1
100613374:      b.ne    0x100613408 <_js_array_grow+0x948>
100613378:      ldr x9, [x8]
10061337c:      cmp x9, x19
100613380:      b.hi    0x100613408 <_js_array_grow+0x948>
100613384:      ldr x9, [x8, #0x8]
100613388:      cmp x9, x19
10061338c:      b.hi    0x1006134a0 <_js_array_grow+0x9e0>
100613390:      b   0x100613408 <_js_array_grow+0x948>
100613394:      mov w8, #0xc                ; =12
100613398:      strb    w8, [sp, #0x38]
10061339c:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006133a0:      add x8, x8, #0x280
1006133a4:      ldapr   x8, [x8]
1006133a8:      cbnz    x8, 0x1006138a4 <_js_array_grow+0xde4>
1006133ac:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006133b0:      ldrb    w8, [x8, #0x288]
1006133b4:      cbz w8, 0x100613600 <_js_array_grow+0xb40>
1006133b8:      adrp    x0, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
1006133bc:      add x0, x0, #0x410
1006133c0:      add x1, sp, #0x38
1006133c4:      bl  0x10012d0f4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
1006133c8:      b   0x100613600 <_js_array_grow+0xb40>
1006133cc:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006133d0:      ldr x25, [x0, #0x8]
1006133d4:      ldr x8, [x26]
1006133d8:      cmn x8, #0x1
1006133dc:      b.ne    0x1006130e8 <_js_array_grow+0x628>
1006133e0:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006133e4:      ldr x19, [x0]
1006133e8:      ldr x8, [x25]
1006133ec:      cbnz    x8, 0x100613104 <_js_array_grow+0x644>
1006133f0:      b   0x100613124 <_js_array_grow+0x664>
1006133f4:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006133f8:      lsr x1, x19, #20
1006133fc:      ldr x8, [x0, #0x10]
100613400:      ldrb    w9, [x8, #0x28]
100613404:      tbnz    w9, #0x0, 0x10061336c <_js_array_grow+0x8ac>
100613408:      ldrb    w9, [x8, #0x58]
10061340c:      cbz w9, 0x10061343c <_js_array_grow+0x97c>
100613410:      ldr x9, [x8, #0x50]
100613414:      cmp x9, x1
100613418:      b.ne    0x10061343c <_js_array_grow+0x97c>
10061341c:      ldr x9, [x8, #0x30]
100613420:      cmp x9, x19
100613424:      b.hi    0x10061343c <_js_array_grow+0x97c>
100613428:      ldr x9, [x8, #0x38]
10061342c:      cmp x9, x19
100613430:      b.ls    0x10061343c <_js_array_grow+0x97c>
100613434:      add x8, x8, #0x30
100613438:      b   0x1006134a0 <_js_array_grow+0x9e0>
10061343c:      ldrb    w9, [x8, #0x88]
100613440:      cbz w9, 0x100613470 <_js_array_grow+0x9b0>
100613444:      ldr x9, [x8, #0x80]
100613448:      cmp x9, x1
10061344c:      b.ne    0x100613470 <_js_array_grow+0x9b0>
100613450:      ldr x9, [x8, #0x60]
100613454:      cmp x9, x19
100613458:      b.hi    0x100613470 <_js_array_grow+0x9b0>
10061345c:      ldr x9, [x8, #0x68]
100613460:      cmp x9, x19
100613464:      b.ls    0x100613470 <_js_array_grow+0x9b0>
100613468:      add x8, x8, #0x60
10061346c:      b   0x1006134a0 <_js_array_grow+0x9e0>
100613470:      ldrb    w9, [x8, #0xb8]
100613474:      cbz w9, 0x1006134ac <_js_array_grow+0x9ec>
100613478:      ldr x9, [x8, #0xb0]
10061347c:      cmp x9, x1
100613480:      b.ne    0x1006134ac <_js_array_grow+0x9ec>
100613484:      ldr x9, [x8, #0x90]
100613488:      cmp x9, x19
10061348c:      b.hi    0x1006134ac <_js_array_grow+0x9ec>
100613490:      ldr x9, [x8, #0x98]
100613494:      cmp x9, x19
100613498:      b.ls    0x1006134ac <_js_array_grow+0x9ec>
10061349c:      add x8, x8, #0x90
1006134a0:      ldrb    w8, [x8, #0x19]
1006134a4:      cmp w8, #0xff
1006134a8:      b.ne    0x1006134b8 <_js_array_grow+0x9f8>
1006134ac:      mov x0, x19
1006134b0:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006134b4:      and w8, w0, #0xff
1006134b8:      cmp w8, #0x3
1006134bc:      b.ne    0x100613600 <_js_array_grow+0xb40>
1006134c0:      ldr x8, [x26]
1006134c4:      cmn x8, #0x1
1006134c8:      b.eq    0x100613514 <_js_array_grow+0xa54>
1006134cc:      mrs x9, TPIDRRO_EL0
1006134d0:      and x9, x9, #0xfffffffffffffff8
1006134d4:      ldr x0, [x9, x8, lsl #3]
1006134d8:      cbz x0, 0x100613514 <_js_array_grow+0xa54>
1006134dc:      lsr x1, x21, #20
1006134e0:      ldr x8, [x0, #0x10]
1006134e4:      ldrb    w9, [x8, #0x28]
1006134e8:      tbz w9, #0x0, 0x100613528 <_js_array_grow+0xa68>
1006134ec:      ldr x9, [x8, #0x20]
1006134f0:      cmp x9, x1
1006134f4:      b.ne    0x100613528 <_js_array_grow+0xa68>
1006134f8:      ldr x9, [x8]
1006134fc:      cmp x9, x21
100613500:      b.hi    0x100613528 <_js_array_grow+0xa68>
100613504:      ldr x9, [x8, #0x8]
100613508:      cmp x9, x21
10061350c:      b.hi    0x1006135c0 <_js_array_grow+0xb00>
100613510:      b   0x100613528 <_js_array_grow+0xa68>
100613514:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100613518:      lsr x1, x21, #20
10061351c:      ldr x8, [x0, #0x10]
100613520:      ldrb    w9, [x8, #0x28]
100613524:      tbnz    w9, #0x0, 0x1006134ec <_js_array_grow+0xa2c>
100613528:      ldrb    w9, [x8, #0x58]
10061352c:      cbz w9, 0x10061355c <_js_array_grow+0xa9c>
100613530:      ldr x9, [x8, #0x50]
100613534:      cmp x9, x1
100613538:      b.ne    0x10061355c <_js_array_grow+0xa9c>
10061353c:      ldr x9, [x8, #0x30]
100613540:      cmp x9, x21
100613544:      b.hi    0x10061355c <_js_array_grow+0xa9c>
100613548:      ldr x9, [x8, #0x38]
10061354c:      cmp x9, x21
100613550:      b.ls    0x10061355c <_js_array_grow+0xa9c>
100613554:      add x8, x8, #0x30
100613558:      b   0x1006135c0 <_js_array_grow+0xb00>
10061355c:      ldrb    w9, [x8, #0x88]
100613560:      cbz w9, 0x100613590 <_js_array_grow+0xad0>
100613564:      ldr x9, [x8, #0x80]
100613568:      cmp x9, x1
10061356c:      b.ne    0x100613590 <_js_array_grow+0xad0>
100613570:      ldr x9, [x8, #0x60]
100613574:      cmp x9, x21
100613578:      b.hi    0x100613590 <_js_array_grow+0xad0>
10061357c:      ldr x9, [x8, #0x68]
100613580:      cmp x9, x21
100613584:      b.ls    0x100613590 <_js_array_grow+0xad0>
100613588:      add x8, x8, #0x60
10061358c:      b   0x1006135c0 <_js_array_grow+0xb00>
100613590:      ldrb    w9, [x8, #0xb8]
100613594:      cbz w9, 0x1006135cc <_js_array_grow+0xb0c>
100613598:      ldr x9, [x8, #0xb0]
10061359c:      cmp x9, x1
1006135a0:      b.ne    0x1006135cc <_js_array_grow+0xb0c>
1006135a4:      ldr x9, [x8, #0x90]
1006135a8:      cmp x9, x21
1006135ac:      b.hi    0x1006135cc <_js_array_grow+0xb0c>
1006135b0:      ldr x9, [x8, #0x98]
1006135b4:      cmp x9, x21
1006135b8:      b.ls    0x1006135cc <_js_array_grow+0xb0c>
1006135bc:      add x8, x8, #0x90
1006135c0:      ldrb    w8, [x8, #0x19]
1006135c4:      cmp w8, #0xff
1006135c8:      b.ne    0x1006135d8 <_js_array_grow+0xb18>
1006135cc:      mov x0, x21
1006135d0:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1006135d4:      and w8, w0, #0xff
1006135d8:      cmp w8, #0x3
1006135dc:      b.ne    0x1006132fc <_js_array_grow+0x83c>
1006135e0:      lsr x28, x21, #12
1006135e4:      add x8, x22, x21
1006135e8:      str x8, [sp, #0x10]
1006135ec:      sub x8, x8, #0x1
1006135f0:      lsr x8, x8, #12
1006135f4:      str x8, [sp, #0x18]
1006135f8:      cmp x28, x8
1006135fc:      b.ls    0x1006136a4 <_js_array_grow+0xbe4>
100613600:      mov x0, x21
100613604:      bl  0x1005cf8e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100613608:      cbz x0, 0x100613820 <_js_array_grow+0xd60>
10061360c:      ldrb    w8, [x0]
100613610:      cmp w8, #0x1
100613614:      b.ne    0x100613820 <_js_array_grow+0xd60>
100613618:      ldrb    w8, [x0, #0x1]
10061361c:      tbz w8, #0x1, 0x100613820 <_js_array_grow+0xd60>
100613620:      str x19, [x0, #0x8]
100613624:      orr w8, w8, #0x80
100613628:      strb    w8, [x0, #0x1]
10061362c:      ldrh    w8, [x0, #0x2]
100613630:      orr w8, w8, #0xc000
100613634:      strh    w8, [x0, #0x2]
100613638:      ldr x8, [x26]
10061363c:      cmn x8, #0x1
100613640:      b.eq    0x100613670 <_js_array_grow+0xbb0>
100613644:      mrs x9, TPIDRRO_EL0
100613648:      and x9, x9, #0xfffffffffffffff8
10061364c:      ldr x8, [x9, x8, lsl #3]
100613650:      cbz x8, 0x100613670 <_js_array_grow+0xbb0>
100613654:      ldr x8, [x8, #0x19e8]
100613658:      cbz x8, 0x100613670 <_js_array_grow+0xbb0>
10061365c:      ldr x9, [x8]
100613660:      cbz x9, 0x100613000 <_js_array_grow+0x540>
100613664:      adrp    x0, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
100613668:      add x0, x0, #0xe0
10061366c:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100613670:      adrp    x0, 0x1010b6000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
100613674:      add x0, x0, #0xd48
100613678:      add x1, sp, #0x20
10061367c:      bl  0x100135cc8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100613680:      mov x0, x19
100613684:      ldp x29, x30, [sp, #0xa0]
100613688:      ldp x20, x19, [sp, #0x90]
10061368c:      ldp x22, x21, [sp, #0x80]
100613690:      ldp x24, x23, [sp, #0x70]
100613694:      ldp x26, x25, [sp, #0x60]
100613698:      ldp x28, x27, [sp, #0x50]
10061369c:      add sp, sp, #0xb0
1006136a0:      ret
1006136a4:      mvn x8, x21
1006136a8:      add x8, x8, x19
1006136ac:      str x8, [sp, #0x8]
1006136b0:      adrp    x23, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006136b4:      add x23, x23, #0x280
1006136b8:      mov w27, #0x6               ; =6
1006136bc:      adrp    x22, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006136c0:      b   0x1006136d4 <_js_array_grow+0xc14>
1006136c4:      ldr x8, [sp, #0x18]
1006136c8:      cmp x28, x8
1006136cc:      add x28, x28, #0x1
1006136d0:      b.eq    0x100613600 <_js_array_grow+0xb40>
1006136d4:      str x28, [sp, #0x38]
1006136d8:      add x1, sp, #0x38
1006136dc:      adrp    x0, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
1006136e0:      add x0, x0, #0x558
1006136e4:      bl  0x1001606c0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3set7HashSetjNtNtCs5gMwpk3Cs4e_13perry_runtime9fast_hash9PtrHasherEEE4withNCNvNtNtB2g_2gc7barrier24dirty_old_page_is_marked0bEB2g_>
1006136e8:      cbz w0, 0x1006136c4 <_js_array_grow+0xc04>
1006136ec:      lsl x9, x28, #12
1006136f0:      cmp x21, x9
1006136f4:      csel    x8, x21, x9, hi
1006136f8:      add x9, x9, #0x1, lsl #12   ; =0x1000
1006136fc:      ldr x10, [sp, #0x10]
100613700:      cmp x10, x9
100613704:      csel    x9, x10, x9, lo
100613708:      cmp x8, x9
10061370c:      b.hs    0x1006136c4 <_js_array_grow+0xc04>
100613710:      sub x10, x19, x21
100613714:      add x8, x10, x8
100613718:      lsr x25, x8, #12
10061371c:      ldr x8, [sp, #0x8]
100613720:      add x8, x8, x9
100613724:      lsr x24, x8, #12
100613728:      cmp x25, x24
10061372c:      b.ls    0x100613740 <_js_array_grow+0xc80>
100613730:      b   0x1006136c4 <_js_array_grow+0xc04>
100613734:      cmp x25, x24
100613738:      add x25, x25, #0x1
10061373c:      b.hs    0x1006136c4 <_js_array_grow+0xc04>
100613740:      strb    w27, [sp, #0x38]
100613744:      ldapr   x8, [x23]
100613748:      cbnz    x8, 0x100613774 <_js_array_grow+0xcb4>
10061374c:      ldrb    w8, [x22, #0x288]
100613750:      cbz w8, 0x100613784 <_js_array_grow+0xcc4>
100613754:      add x1, sp, #0x38
100613758:      adrp    x0, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
10061375c:      add x0, x0, #0x410
100613760:      bl  0x10012d0f4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
100613764:      ldr x8, [x26]
100613768:      cmn x8, #0x1
10061376c:      b.ne    0x100613790 <_js_array_grow+0xcd0>
100613770:      b   0x1006137bc <_js_array_grow+0xcfc>
100613774:      mov x0, x23
100613778:      bl  0x100cd0054 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
10061377c:      ldrb    w8, [x22, #0x288]
100613780:      cbnz    w8, 0x100613754 <_js_array_grow+0xc94>
100613784:      ldr x8, [x26]
100613788:      cmn x8, #0x1
10061378c:      b.eq    0x1006137bc <_js_array_grow+0xcfc>
100613790:      mrs x9, TPIDRRO_EL0
100613794:      and x9, x9, #0xfffffffffffffff8
100613798:      ldr x0, [x9, x8, lsl #3]
10061379c:      cbz x0, 0x1006137bc <_js_array_grow+0xcfc>
1006137a0:      and x8, x25, #0xf
1006137a4:      add x8, x0, x8, lsl #3
1006137a8:      ldr x8, [x8, #0x88]
1006137ac:      cmp x25, x8
1006137b0:      b.ne    0x1006137d4 <_js_array_grow+0xd14>
1006137b4:      mov w8, #0x9                ; =9
1006137b8:      b   0x1006137e4 <_js_array_grow+0xd24>
1006137bc:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1006137c0:      and x8, x25, #0xf
1006137c4:      add x8, x0, x8, lsl #3
1006137c8:      ldr x8, [x8, #0x88]
1006137cc:      cmp x25, x8
1006137d0:      b.eq    0x1006137b4 <_js_array_grow+0xcf4>
1006137d4:      mov x0, x25
1006137d8:      bl  0x1005b96e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier28mark_dirty_old_page_uncached>
1006137dc:      tbz w0, #0x0, 0x100613734 <_js_array_grow+0xc74>
1006137e0:      mov w8, #0x7                ; =7
1006137e4:      strb    w8, [sp, #0x38]
1006137e8:      ldapr   x8, [x23]
1006137ec:      cbnz    x8, 0x10061380c <_js_array_grow+0xd4c>
1006137f0:      ldrb    w8, [x22, #0x288]
1006137f4:      cbz w8, 0x100613734 <_js_array_grow+0xc74>
1006137f8:      add x1, sp, #0x38
1006137fc:      adrp    x0, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
100613800:      add x0, x0, #0x410
100613804:      bl  0x10012d0f4 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc9telemetry20BarrierTraceCountersEE4withNCNvNtB1w_7barrier32bump_write_barrier_trace_counter0uEB1y_>
100613808:      b   0x100613734 <_js_array_grow+0xc74>
10061380c:      mov x0, x23
100613810:      bl  0x100cd0054 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
100613814:      ldrb    w8, [x22, #0x288]
100613818:      cbnz    w8, 0x1006137f8 <_js_array_grow+0xd38>
10061381c:      b   0x100613734 <_js_array_grow+0xc74>
100613820:      str x21, [sp, #0x30]
100613824:      add x8, sp, #0x30
100613828:      adrp    x9, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
10061382c:      add x9, x9, #0x288
100613830:      stp x8, x9, [sp, #0x38]
100613834:      adrp    x0, 0x100de6000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.631+0x1e9>
100613838:      add x0, x0, #0x8da
10061383c:      adrp    x2, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
100613840:      add x2, x2, #0xd40
100613844:      add x1, sp, #0x38
100613848:      bl  0x100c99efc <__RNvNtCsjgY6bXVaRmE_4core9panicking9panic_fmt>
10061384c:      mov x19, x1
100613850:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100613854:      mov x1, x19
100613858:      b   0x10061319c <_js_array_grow+0x6dc>
10061385c:      adrp    x0, 0x1010b6000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
100613860:      add x0, x0, #0xe40
100613864:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100613868:      adrp    x0, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
10061386c:      add x0, x0, #0x58
100613870:      bl  0x100cd0144 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
100613874:      adrp    x8, 0x101131000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7bun_ffi8callback17PENDING_CALLBACKS>
100613878:      ldrb    w8, [x8, #0x60]
10061387c:      cbnz    w8, 0x100613324 <_js_array_grow+0x864>
100613880:      b   0x100613600 <_js_array_grow+0xb40>
100613884:      adrp    x0, 0x1010b6000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
100613888:      add x0, x0, #0xdc8
10061388c:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100613890:      bl  0x100cb753c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100613894:      adrp    x0, 0x100dd8000 <_anon.6ab1a69beca0dab646726cb1e935b5ae.447+0x2c7>
100613898:      add x0, x0, #0x295
10061389c:      mov w1, #0xb                ; =11
1006138a0:      bl  0x100cb7504 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1006138a4:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006138a8:      add x0, x0, #0x280
1006138ac:      bl  0x100cd0054 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_trace_enabled0E0zEB1A_>
1006138b0:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1006138b4:      ldrb    w8, [x8, #0x288]
1006138b8:      cbnz    w8, 0x1006133b8 <_js_array_grow+0x8f8>
1006138bc:      b   0x100613600 <_js_array_grow+0xb40>
1006138c0:      adrp    x2, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
1006138c4:      add x2, x2, #0x5b8
1006138c8:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1006138cc:      adrp    x2, 0x1010b7000 <_anon.e2662357639f7b4d822a0c1c5c4e564f.65+0x190>
1006138d0:      add x2, x2, #0x5d0
1006138d4:      mov x1, x8
1006138d8:      bl  0x100c99d8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
