/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100843a80 <_js_array_grow>:
100843a80:      sub sp, sp, #0x90
100843a84:      stp x28, x27, [sp, #0x30]
100843a88:      stp x26, x25, [sp, #0x40]
100843a8c:      stp x24, x23, [sp, #0x50]
100843a90:      stp x22, x21, [sp, #0x60]
100843a94:      stp x20, x19, [sp, #0x70]
100843a98:      stp x29, x30, [sp, #0x80]
100843a9c:      add x29, sp, #0x80
100843aa0:      cmp x0, #0xfff
100843aa4:      b.ls    0x100843df4 <_js_array_grow+0x374>
100843aa8:      mov x19, x0
100843aac:      lsr x8, x0, #51
100843ab0:      cmp x8, #0xfff
100843ab4:      b.lo    0x100843acc <_js_array_grow+0x4c>
100843ab8:      mov w8, #0x7ffc             ; =32764
100843abc:      cmp x8, x19, lsr #48
100843ac0:      b.eq    0x100843df4 <_js_array_grow+0x374>
100843ac4:      ands    x19, x19, #0xffffffffffff
100843ac8:      b.eq    0x100843df4 <_js_array_grow+0x374>
100843acc:      and x8, x19, #0xfffffffffff00000
100843ad0:      lsr x9, x19, #47
100843ad4:      cmp x9, #0x0
100843ad8:      ccmp    x8, #0x0, #0x4, eq
100843adc:      b.eq    0x100843df4 <_js_array_grow+0x374>
100843ae0:      tst x19, #0x3
100843ae4:      ccmp    x19, #0x7, #0x0, eq
100843ae8:      mov x22, x1
100843aec:      b.ls    0x100843bf4 <_js_array_grow+0x174>
100843af0:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
100843af4:      add x8, x8, #0x2c8
100843af8:      ldr x8, [x8]
100843afc:      cmn x8, #0x1
100843b00:      b.eq    0x100843fd8 <_js_array_grow+0x558>
100843b04:      mrs x9, TPIDRRO_EL0
100843b08:      and x9, x9, #0xfffffffffffffff8
100843b0c:      ldr x0, [x9, x8, lsl #3]
100843b10:      cbz x0, 0x100843fd8 <_js_array_grow+0x558>
100843b14:      lsr x1, x19, #20
100843b18:      ldr x8, [x0, #0x10]
100843b1c:      ldrb    w9, [x8, #0x28]
100843b20:      tbz w9, #0x0, 0x100843b40 <_js_array_grow+0xc0>
100843b24:      ldr x9, [x8, #0x20]
100843b28:      cmp x9, x1
100843b2c:      b.ne    0x100843b40 <_js_array_grow+0xc0>
100843b30:      ldp x9, x10, [x8]
100843b34:      cmp x9, x19
100843b38:      ccmp    x10, x19, #0x0, ls
100843b3c:      b.hi    0x100843bbc <_js_array_grow+0x13c>
100843b40:      ldrb    w9, [x8, #0x58]
100843b44:      cbz w9, 0x100843b64 <_js_array_grow+0xe4>
100843b48:      ldr x9, [x8, #0x50]
100843b4c:      cmp x9, x1
100843b50:      b.ne    0x100843b64 <_js_array_grow+0xe4>
100843b54:      ldp x9, x10, [x8, #0x30]
100843b58:      cmp x9, x19
100843b5c:      ccmp    x10, x19, #0x0, ls
100843b60:      b.hi    0x100843bb0 <_js_array_grow+0x130>
100843b64:      ldrb    w9, [x8, #0x88]
100843b68:      cbz w9, 0x100843b88 <_js_array_grow+0x108>
100843b6c:      ldr x9, [x8, #0x80]
100843b70:      cmp x9, x1
100843b74:      b.ne    0x100843b88 <_js_array_grow+0x108>
100843b78:      ldp x9, x10, [x8, #0x60]
100843b7c:      cmp x9, x19
100843b80:      ccmp    x10, x19, #0x0, ls
100843b84:      b.hi    0x100843bb8 <_js_array_grow+0x138>
100843b88:      ldrb    w9, [x8, #0xb8]
100843b8c:      cbz w9, 0x100843bc8 <_js_array_grow+0x148>
100843b90:      ldr x9, [x8, #0xb0]
100843b94:      cmp x9, x1
100843b98:      b.ne    0x100843bc8 <_js_array_grow+0x148>
100843b9c:      ldp x9, x10, [x8, #0x90]!
100843ba0:      cmp x9, x19
100843ba4:      ccmp    x10, x19, #0x0, ls
100843ba8:      b.hi    0x100843bbc <_js_array_grow+0x13c>
100843bac:      b   0x100843bc8 <_js_array_grow+0x148>
100843bb0:      add x8, x8, #0x30
100843bb4:      b   0x100843bbc <_js_array_grow+0x13c>
100843bb8:      add x8, x8, #0x60
100843bbc:      ldrb    w8, [x8, #0x19]
100843bc0:      cmp w8, #0xff
100843bc4:      b.ne    0x100843bd4 <_js_array_grow+0x154>
100843bc8:      mov x0, x19
100843bcc:      bl  0x100559228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100843bd0:      and w8, w0, #0xff
100843bd4:      cbz w8, 0x100843bf4 <_js_array_grow+0x174>
100843bd8:      ldurb   w8, [x19, #-0x8]
100843bdc:      ldurb   w9, [x19, #-0x7]
100843be0:      mov w10, #0x82              ; =130
100843be4:      and w9, w9, w10
100843be8:      cmp w9, #0x2
100843bec:      ccmp    w8, #0x1, #0x0, eq
100843bf0:      b.eq    0x100843e18 <_js_array_grow+0x398>
100843bf4:      mov x0, x19
100843bf8:      bl  0x10080ca88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100843bfc:      mov x8, x0
100843c00:      cbz x0, 0x100843c9c <_js_array_grow+0x21c>
100843c04:      ldrb    w9, [x8]
100843c08:      cmp w9, #0x1
100843c0c:      b.ne    0x100843d2c <_js_array_grow+0x2ac>
100843c10:      ldrsb   w9, [x8, #0x1]
100843c14:      mov x0, x8
100843c18:      tbz w9, #0x1f, 0x100843d70 <_js_array_grow+0x2f0>
100843c1c:      mov x20, x8
100843c20:      ldr x19, [x8, #0x8]
100843c24:      mov x0, x19
100843c28:      bl  0x10080ca88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100843c2c:      mov x1, x22
100843c30:      cbz x0, 0x100843df4 <_js_array_grow+0x374>
100843c34:      ldrb    w8, [x0]
100843c38:      cmp w8, #0x1
100843c3c:      b.ne    0x100843df4 <_js_array_grow+0x374>
100843c40:      ldrsb   w8, [x0, #0x1]
100843c44:      tbz w8, #0x1f, 0x100843d24 <_js_array_grow+0x2a4>
100843c48:      mov w21, #0x1               ; =1
100843c4c:      ldr x19, [x0, #0x8]
100843c50:      mov x0, x19
100843c54:      bl  0x10080ca88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100843c58:      mov x1, x22
100843c5c:      cbz x0, 0x100843df4 <_js_array_grow+0x374>
100843c60:      ldrb    w8, [x0]
100843c64:      cmp w8, #0x1
100843c68:      b.ne    0x100843df4 <_js_array_grow+0x374>
100843c6c:      cmp w21, #0x3f
100843c70:      b.hi    0x100843df4 <_js_array_grow+0x374>
100843c74:      add w21, w21, #0x1
100843c78:      ldrsb   w8, [x0, #0x1]
100843c7c:      tbnz    w8, #0x1f, 0x100843c4c <_js_array_grow+0x1cc>
100843c80:      mov x8, x20
100843c84:      str x19, [x20, #0x8]
100843c88:      ldrb    w9, [x20, #0x1]
100843c8c:      orr w9, w9, #0x80
100843c90:      strb    w9, [x20, #0x1]
100843c94:      ldrb    w9, [x0]
100843c98:      b   0x100843d34 <_js_array_grow+0x2b4>
100843c9c:      mov x20, x8
100843ca0:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
100843ca4:      add x8, x8, #0x71b
100843ca8:      ldaprb  w8, [x8]
100843cac:      cbz w8, 0x100843cdc <_js_array_grow+0x25c>
100843cb0:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
100843cb4:      add x8, x8, #0x2d0
100843cb8:      ldapr   x9, [x8]
100843cbc:      cmp x9, x19
100843cc0:      b.hi    0x100843cdc <_js_array_grow+0x25c>
100843cc4:      ldapur  x8, [x8, #0x8]
100843cc8:      cmp x8, x19
100843ccc:      b.lo    0x100843cdc <_js_array_grow+0x25c>
100843cd0:      mov x0, x19
100843cd4:      bl  0x10019d27c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100843cd8:      tbnz    w0, #0x0, 0x100843d20 <_js_array_grow+0x2a0>
100843cdc:      adrp    x8, 0x1011f9000 <_PERRY_TA_KIND_CACHE+0x10>
100843ce0:      add x8, x8, #0x478
100843ce4:      ldaprb  w8, [x8]
100843ce8:      mov x1, x22
100843cec:      cbz w8, 0x100843df4 <_js_array_grow+0x374>
100843cf0:      adrp    x8, 0x10112d000 <__RNvNvNtCs5gMwpk3Cs4e_13perry_runtime13cluster_sched12worker_state2WS+0xa8>
100843cf4:      add x8, x8, #0x4b8
100843cf8:      ldapr   x9, [x8]
100843cfc:      cmp x9, x19
100843d00:      b.hi    0x100843df4 <_js_array_grow+0x374>
100843d04:      ldapur  x8, [x8, #0x8]
100843d08:      cmp x8, x19
100843d0c:      b.lo    0x100843df4 <_js_array_grow+0x374>
100843d10:      mov x0, x19
100843d14:      bl  0x10063ed2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100843d18:      mov x1, x22
100843d1c:      tbz w0, #0x0, 0x100843df4 <_js_array_grow+0x374>
100843d20:      mov x0, #0x0                ; =0
100843d24:      mov x8, x20
100843d28:      b   0x100843d70 <_js_array_grow+0x2f0>
100843d2c:      mov x0, x8
100843d30:      mov x1, x22
100843d34:      cmp w9, #0x1
100843d38:      b.eq    0x100843d70 <_js_array_grow+0x2f0>
100843d3c:      cmp w9, #0x9
100843d40:      b.ne    0x100843df4 <_js_array_grow+0x374>
100843d44:      ldr w8, [x19, #0x4]
100843d48:      mov w9, #0x5841             ; =22593
100843d4c:      movk    w9, #0x4c5a, lsl #16
100843d50:      cmp w8, w9
100843d54:      b.ne    0x100843df4 <_js_array_grow+0x374>
100843d58:      mov x0, x19
100843d5c:      bl  0x1008b9a34 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
100843d60:      mov x1, x22
100843d64:      cbz x0, 0x100843df4 <_js_array_grow+0x374>
100843d68:      mov x19, x0
100843d6c:      b   0x100843e3c <_js_array_grow+0x3bc>
100843d70:      ldp w10, w9, [x19]
100843d74:      cmp w10, w9
100843d78:      b.ls    0x100843d98 <_js_array_grow+0x318>
100843d7c:      cbz x8, 0x100843da8 <_js_array_grow+0x328>
100843d80:      ldr w8, [x0, #0x4]
100843d84:      lsl x9, x9, #3
100843d88:      add x9, x9, #0x10
100843d8c:      cmp x9, x8
100843d90:      b.ne    0x100843da8 <_js_array_grow+0x328>
100843d94:      b   0x100843e3c <_js_array_grow+0x3bc>
100843d98:      mov w8, #0xe100             ; =57600
100843d9c:      movk    w8, #0x5f5, lsl #16
100843da0:      cmp w10, w8
100843da4:      b.ls    0x100843e3c <_js_array_grow+0x3bc>
100843da8:      adrp    x8, 0x101178000 <_out_buf+0x3f08>
100843dac:      add x8, x8, #0x71b
100843db0:      ldaprb  w8, [x8]
100843db4:      cbz w8, 0x100843de4 <_js_array_grow+0x364>
100843db8:      adrp    x8, 0x10112c000 <_perry_global_baseline_worker_ts__1>
100843dbc:      add x8, x8, #0x2d0
100843dc0:      ldapr   x9, [x8]
100843dc4:      cmp x9, x19
100843dc8:      b.hi    0x100843de4 <_js_array_grow+0x364>
100843dcc:      ldapur  x8, [x8, #0x8]
100843dd0:      cmp x8, x19
100843dd4:      b.lo    0x100843de4 <_js_array_grow+0x364>
100843dd8:      mov x0, x19
100843ddc:      bl  0x10019d27c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100843de0:      tbnz    w0, #0x0, 0x100843e3c <_js_array_grow+0x3bc>
100843de4:      mov x0, x19
100843de8:      bl  0x1007ba4f4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
100843dec:      mov x1, x22
100843df0:      tbnz    w0, #0x0, 0x100843e3c <_js_array_grow+0x3bc>
100843df4:      mov x0, x1
100843df8:      ldp x29, x30, [sp, #0x80]
100843dfc:      ldp x20, x19, [sp, #0x70]
100843e00:      ldp x22, x21, [sp, #0x60]
100843e04:      ldp x24, x23, [sp, #0x50]
100843e08:      ldp x26, x25, [sp, #0x40]
100843e0c:      ldp x28, x27, [sp, #0x30]
100843e10:      add sp, sp, #0x90
100843e14:      b   0x10091817c <_js_array_alloc>
100843e18:      ldr w8, [x19]
100843e1c:      mov w9, #0xe100             ; =57600
100843e20:      movk    w9, #0x5f5, lsl #16
100843e24:      orr w9, w9, #0x1
100843e28:      cmp w8, w9
100843e2c:      b.hs    0x100843bf4 <_js_array_grow+0x174>
100843e30:      ldr w9, [x19, #0x4]
100843e34:      cmp w8, w9
100843e38:      b.hi    0x100843bf4 <_js_array_grow+0x174>
100843e3c:      mov x0, x19
100843e40:      bl  0x1007f404c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100843e44:      tst w0, #0x6
100843e48:      b.ne    0x100844344 <_js_array_grow+0x8c4>
100843e4c:      mov x0, x19
100843e50:      bl  0x1007f404c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header18array_object_flags>
100843e54:      tbnz    w0, #0x0, 0x100844344 <_js_array_grow+0x8c4>
100843e58:      adrp    x23, 0x10112c000 <_perry_global_baseline_worker_ts__1>
100843e5c:      add x23, x23, #0x2c8
100843e60:      ldr x8, [x23]
100843e64:      cmn x8, #0x1
100843e68:      b.eq    0x100843e9c <_js_array_grow+0x41c>
100843e6c:      mrs x9, TPIDRRO_EL0
100843e70:      and x9, x9, #0xfffffffffffffff8
100843e74:      ldr x8, [x9, x8, lsl #3]
100843e78:      cbz x8, 0x100843e9c <_js_array_grow+0x41c>
100843e7c:      ldr x8, [x8, #0x19e8]
100843e80:      cbz x8, 0x100843e9c <_js_array_grow+0x41c>
100843e84:      ldr x9, [x8]
100843e88:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100843e8c:      cmp x9, x10
100843e90:      b.hs    0x1008443cc <_js_array_grow+0x94c>
100843e94:      ldr x20, [x8, #0x18]
100843e98:      b   0x100843eac <_js_array_grow+0x42c>
100843e9c:      adrp    x0, 0x1010c9000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1798+0x220>
100843ea0:      add x0, x0, #0xdf0
100843ea4:      bl  0x1001357ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100843ea8:      mov x20, x0
100843eac:      str x20, [sp]
100843eb0:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100843eb4:      stp x19, x8, [sp, #0x20]
100843eb8:      mov w8, #0x1                ; =1
100843ebc:      str x8, [sp, #0x18]
100843ec0:      add x0, sp, #0x18
100843ec4:      bl  0x1007b9d94 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100843ec8:      str x0, [sp, #0x8]
100843ecc:      ldr w24, [x19, #0x4]
100843ed0:      cmp w22, w24
100843ed4:      b.ls    0x100843f94 <_js_array_grow+0x514>
100843ed8:      mov x21, x0
100843edc:      lsl w8, w24, #1
100843ee0:      cmp w22, w8
100843ee4:      csel    w25, w22, w8, hi
100843ee8:      ubfiz   x22, x25, #3, #32
100843eec:      ldurb   w8, [x19, #-0x7]
100843ef0:      tbnz    w8, #0x5, 0x1008441b0 <_js_array_grow+0x730>
100843ef4:      ldr x8, [x23]
100843ef8:      cmn x8, #0x1
100843efc:      b.eq    0x100844188 <_js_array_grow+0x708>
100843f00:      mrs x9, TPIDRRO_EL0
100843f04:      and x9, x9, #0xfffffffffffffff8
100843f08:      ldr x0, [x9, x8, lsl #3]
100843f0c:      cbz x0, 0x100844188 <_js_array_grow+0x708>
100843f10:      lsr x1, x19, #20
100843f14:      ldr x8, [x0, #0x10]
100843f18:      ldrb    w9, [x8, #0x28]
100843f1c:      tbz w9, #0x0, 0x100843f3c <_js_array_grow+0x4bc>
100843f20:      ldr x9, [x8, #0x20]
100843f24:      cmp x9, x1
100843f28:      b.ne    0x100843f3c <_js_array_grow+0x4bc>
100843f2c:      ldp x9, x10, [x8]
100843f30:      cmp x9, x19
100843f34:      ccmp    x10, x19, #0x0, ls
100843f38:      b.hi    0x100844028 <_js_array_grow+0x5a8>
100843f3c:      ldrb    w9, [x8, #0x58]
100843f40:      cbz w9, 0x100843f60 <_js_array_grow+0x4e0>
100843f44:      ldr x9, [x8, #0x50]
100843f48:      cmp x9, x1
100843f4c:      b.ne    0x100843f60 <_js_array_grow+0x4e0>
100843f50:      ldp x9, x10, [x8, #0x30]
100843f54:      cmp x9, x19
100843f58:      ccmp    x10, x19, #0x0, ls
100843f5c:      b.hi    0x100844024 <_js_array_grow+0x5a4>
100843f60:      ldrb    w9, [x8, #0x88]
100843f64:      cbz w9, 0x100843ff0 <_js_array_grow+0x570>
100843f68:      ldr x9, [x8, #0x80]
100843f6c:      cmp x9, x1
100843f70:      b.ne    0x100843ff0 <_js_array_grow+0x570>
100843f74:      ldr x9, [x8, #0x60]
100843f78:      cmp x9, x19
100843f7c:      b.hi    0x100843ff0 <_js_array_grow+0x570>
100843f80:      ldr x9, [x8, #0x68]
100843f84:      cmp x9, x19
100843f88:      b.ls    0x100843ff0 <_js_array_grow+0x570>
100843f8c:      add x8, x8, #0x60
100843f90:      b   0x100844028 <_js_array_grow+0x5a8>
100843f94:      ldr x8, [x23]
100843f98:      cmn x8, #0x1
100843f9c:      b.eq    0x100844334 <_js_array_grow+0x8b4>
100843fa0:      mrs x9, TPIDRRO_EL0
100843fa4:      and x9, x9, #0xfffffffffffffff8
100843fa8:      ldr x8, [x9, x8, lsl #3]
100843fac:      cbz x8, 0x100844334 <_js_array_grow+0x8b4>
100843fb0:      ldr x8, [x8, #0x19e8]
100843fb4:      cbz x8, 0x100844334 <_js_array_grow+0x8b4>
100843fb8:      ldr x9, [x8]
100843fbc:      cbnz    x9, 0x100844328 <_js_array_grow+0x8a8>
100843fc0:      ldr x9, [x8, #0x18]
100843fc4:      cmp x20, x9
100843fc8:      b.hi    0x100843fd0 <_js_array_grow+0x550>
100843fcc:      str x20, [x8, #0x18]
100843fd0:      str xzr, [x8]
100843fd4:      b   0x100844344 <_js_array_grow+0x8c4>
100843fd8:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100843fdc:      lsr x1, x19, #20
100843fe0:      ldr x8, [x0, #0x10]
100843fe4:      ldrb    w9, [x8, #0x28]
100843fe8:      tbnz    w9, #0x0, 0x100843b24 <_js_array_grow+0xa4>
100843fec:      b   0x100843b40 <_js_array_grow+0xc0>
100843ff0:      ldrb    w9, [x8, #0xb8]
100843ff4:      cbz w9, 0x100844034 <_js_array_grow+0x5b4>
100843ff8:      ldr x9, [x8, #0xb0]
100843ffc:      cmp x9, x1
100844000:      b.ne    0x100844034 <_js_array_grow+0x5b4>
100844004:      ldr x9, [x8, #0x90]
100844008:      cmp x9, x19
10084400c:      b.hi    0x100844034 <_js_array_grow+0x5b4>
100844010:      ldr x9, [x8, #0x98]
100844014:      cmp x9, x19
100844018:      b.ls    0x100844034 <_js_array_grow+0x5b4>
10084401c:      add x8, x8, #0x90
100844020:      b   0x100844028 <_js_array_grow+0x5a8>
100844024:      add x8, x8, #0x30
100844028:      ldrb    w8, [x8, #0x19]
10084402c:      cmp w8, #0xff
100844030:      b.ne    0x100844040 <_js_array_grow+0x5c0>
100844034:      mov x0, x19
100844038:      bl  0x100559228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
10084403c:      and w8, w0, #0xff
100844040:      cmp w8, #0x1
100844044:      b.ne    0x1008441b0 <_js_array_grow+0x730>
100844048:      mov w8, #0x3ffe             ; =16382
10084404c:      cmp w25, w8
100844050:      b.hi    0x1008441b0 <_js_array_grow+0x730>
100844054:      ldr x8, [x23]
100844058:      cmn x8, #0x1
10084405c:      b.eq    0x1008441a0 <_js_array_grow+0x720>
100844060:      mrs x9, TPIDRRO_EL0
100844064:      and x9, x9, #0xfffffffffffffff8
100844068:      ldr x0, [x9, x8, lsl #3]
10084406c:      cbz x0, 0x1008441a0 <_js_array_grow+0x720>
100844070:      ldr x8, [x0, #0x28]
100844074:      ldrb    w8, [x8]
100844078:      tbnz    w8, #0x0, 0x1008441b0 <_js_array_grow+0x730>
10084407c:      ldr x8, [x23]
100844080:      cmn x8, #0x1
100844084:      b.eq    0x100844368 <_js_array_grow+0x8e8>
100844088:      mrs x9, TPIDRRO_EL0
10084408c:      and x9, x9, #0xfffffffffffffff8
100844090:      ldr x0, [x9, x8, lsl #3]
100844094:      cbz x0, 0x100844368 <_js_array_grow+0x8e8>
100844098:      ldr x26, [x0, #0x8]
10084409c:      ldr x8, [x23]
1008440a0:      cmn x8, #0x1
1008440a4:      b.eq    0x10084437c <_js_array_grow+0x8fc>
1008440a8:      mrs x9, TPIDRRO_EL0
1008440ac:      and x9, x9, #0xfffffffffffffff8
1008440b0:      ldr x0, [x9, x8, lsl #3]
1008440b4:      cbz x0, 0x10084437c <_js_array_grow+0x8fc>
1008440b8:      ldr x19, [x0]
1008440bc:      ldr x8, [x26]
1008440c0:      cbz x8, 0x1008440e4 <_js_array_grow+0x664>
1008440c4:      ldp x1, x0, [x19, #0x10]
1008440c8:      cmp x0, x1
1008440cc:      b.hs    0x1008443f8 <_js_array_grow+0x978>
1008440d0:      ldr x8, [x19, #0x8]
1008440d4:      ldr x9, [x26, #0x8]
1008440d8:      mov w10, #0x30              ; =48
1008440dc:      madd    x8, x0, x10, x8
1008440e0:      str x9, [x8, #0x20]
1008440e4:      add x27, x22, #0x10
1008440e8:      ldr x1, [x19, #0x18]
1008440ec:      add x2, x22, #0x10
1008440f0:      mov x0, x19
1008440f4:      bl  0x1007b8390 <__RNvMs1_NtNtCs5gMwpk3Cs4e_13perry_runtime5arena5blockNtB5_5Arena15try_block_alloc>
1008440f8:      cmp x0, #0x1
1008440fc:      b.ne    0x1008441b0 <_js_array_grow+0x730>
100844100:      ldr x8, [x26]
100844104:      cbz x8, 0x100844134 <_js_array_grow+0x6b4>
100844108:      ldp x8, x0, [x19, #0x10]
10084410c:      cmp x0, x8
100844110:      b.hs    0x100844404 <_js_array_grow+0x984>
100844114:      ldr x8, [x19, #0x8]
100844118:      mov w9, #0x30               ; =48
10084411c:      madd    x8, x0, x9, x8
100844120:      ldr x9, [x8, #0x10]
100844124:      ldur    q0, [x8, #0x18]
100844128:      str x9, [x26]
10084412c:      ext.16b v0, v0, v0, #0x8
100844130:      stur    q0, [x26, #0x8]
100844134:      cbz x1, 0x1008441b0 <_js_array_grow+0x730>
100844138:      mov w8, #0x1                ; =1
10084413c:      strb    w8, [x1]
100844140:      ldr x8, [x23]
100844144:      cmn x8, #0x1
100844148:      b.eq    0x1008443bc <_js_array_grow+0x93c>
10084414c:      mrs x9, TPIDRRO_EL0
100844150:      and x9, x9, #0xfffffffffffffff8
100844154:      ldr x0, [x9, x8, lsl #3]
100844158:      cbz x0, 0x1008443bc <_js_array_grow+0x93c>
10084415c:      ldr x8, [x0, #0x30]
100844160:      ldrb    w8, [x8]
100844164:      orr w8, w8, #0x2
100844168:      strb    w8, [x1, #0x1]
10084416c:      mov x0, x1
100844170:      mov x19, x1
100844174:      bl  0x1007ecc18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier19gc_note_black_birth>
100844178:      strh    wzr, [x19, #0x2]
10084417c:      str w27, [x19, #0x4]
100844180:      add x19, x19, #0x8
100844184:      b   0x1008441d0 <_js_array_grow+0x750>
100844188:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10084418c:      lsr x1, x19, #20
100844190:      ldr x8, [x0, #0x10]
100844194:      ldrb    w9, [x8, #0x28]
100844198:      tbnz    w9, #0x0, 0x100843f20 <_js_array_grow+0x4a0>
10084419c:      b   0x100843f3c <_js_array_grow+0x4bc>
1008441a0:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008441a4:      ldr x8, [x0, #0x28]
1008441a8:      ldrb    w8, [x8]
1008441ac:      tbz w8, #0x0, 0x10084407c <_js_array_grow+0x5fc>
1008441b0:      add x0, x22, #0x8
1008441b4:      mov w1, #0x8                ; =8
1008441b8:      mov w2, #0x1                ; =1
1008441bc:      bl  0x10070cbc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena10allocators18arena_alloc_gc_old>
1008441c0:      mov x19, x0
1008441c4:      ldurb   w8, [x0, #-0x7]
1008441c8:      orr w8, w8, #0x20
1008441cc:      sturb   w8, [x0, #-0x7]
1008441d0:      ldr x8, [x23]
1008441d4:      cmn x8, #0x1
1008441d8:      b.eq    0x10084423c <_js_array_grow+0x7bc>
1008441dc:      mrs x9, TPIDRRO_EL0
1008441e0:      and x9, x9, #0xfffffffffffffff8
1008441e4:      ldr x8, [x9, x8, lsl #3]
1008441e8:      cbz x8, 0x10084423c <_js_array_grow+0x7bc>
1008441ec:      ldr x8, [x8, #0x19e8]
1008441f0:      cbz x8, 0x10084423c <_js_array_grow+0x7bc>
1008441f4:      ldr x9, [x8]
1008441f8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008441fc:      cmp x9, x10
100844200:      b.hs    0x1008443d8 <_js_array_grow+0x958>
100844204:      add x10, x9, #0x1
100844208:      str x10, [x8]
10084420c:      ldr x10, [x8, #0x18]
100844210:      cmp x21, x10
100844214:      b.hs    0x1008443e4 <_js_array_grow+0x964>
100844218:      ldr x10, [x8, #0x10]
10084421c:      mov w11, #0x18              ; =24
100844220:      madd    x10, x21, x11, x10
100844224:      ldr x11, [x10]
100844228:      cmp x11, #0x1
10084422c:      b.ne    0x1008443e8 <_js_array_grow+0x968>
100844230:      ldr x21, [x10, #0x8]
100844234:      str x9, [x8]
100844238:      b   0x100844250 <_js_array_grow+0x7d0>
10084423c:      adrp    x0, 0x1010c9000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1798+0x220>
100844240:      add x0, x0, #0xdf0
100844244:      add x1, sp, #0x8
100844248:      bl  0x1001355d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10084424c:      mov x21, x0
100844250:      lsl x8, x24, #3
100844254:      add x22, x8, #0x8
100844258:      mov x0, x19
10084425c:      mov x1, x21
100844260:      mov x2, x22
100844264:      bl  0x100ce4f6c <_writev+0x100ce4f6c>
100844268:      str w25, [x19, #0x4]
10084426c:      sub x8, x25, x24
100844270:      lsl x2, x8, #3
100844274:      adrp    x1, 0x100e17000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.1076+0xdb>
100844278:      add x1, x1, #0x3b0
10084427c:      add x0, x19, x22
100844280:      bl  0x100ce4f90 <_writev+0x100ce4f90>
100844284:      ldurh   w8, [x21, #-0x6]
100844288:      sturh   w8, [x19, #-0x6]
10084428c:      mov x0, x21
100844290:      mov x1, x19
100844294:      bl  0x1007e99dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout15layout_transfer>
100844298:      mov x0, x21
10084429c:      mov x1, x19
1008442a0:      bl  0x1008e17ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35transfer_array_named_property_owner>
1008442a4:      mov x0, x21
1008442a8:      mov x1, x19
1008442ac:      bl  0x1004035f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object16descriptor_state25transfer_descriptor_owner>
1008442b0:      mov x0, x19
1008442b4:      mov x1, x21
1008442b8:      mov x2, x19
1008442bc:      mov x3, x22
1008442c0:      bl  0x10055554c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier38relocate_copied_old_object_dirty_pages>
1008442c4:      tbnz    w0, #0x0, 0x1008442d0 <_js_array_grow+0x850>
1008442c8:      mov x0, x19
1008442cc:      bl  0x1007f1380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array15header_gc_slots34replay_array_growth_write_barriers>
1008442d0:      mov x0, x21
1008442d4:      bl  0x10080ca88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008442d8:      cbz x0, 0x100844390 <_js_array_grow+0x910>
1008442dc:      ldrb    w8, [x0]
1008442e0:      cmp w8, #0x1
1008442e4:      b.ne    0x100844390 <_js_array_grow+0x910>
1008442e8:      ldrb    w8, [x0, #0x1]
1008442ec:      tbz w8, #0x1, 0x100844390 <_js_array_grow+0x910>
1008442f0:      str x19, [x0, #0x8]
1008442f4:      orr w8, w8, #0x80
1008442f8:      strb    w8, [x0, #0x1]
1008442fc:      ldr x8, [x23]
100844300:      cmn x8, #0x1
100844304:      b.eq    0x100844334 <_js_array_grow+0x8b4>
100844308:      mrs x9, TPIDRRO_EL0
10084430c:      and x9, x9, #0xfffffffffffffff8
100844310:      ldr x8, [x9, x8, lsl #3]
100844314:      cbz x8, 0x100844334 <_js_array_grow+0x8b4>
100844318:      ldr x8, [x8, #0x19e8]
10084431c:      cbz x8, 0x100844334 <_js_array_grow+0x8b4>
100844320:      ldr x9, [x8]
100844324:      cbz x9, 0x100843fc0 <_js_array_grow+0x540>
100844328:      adrp    x0, 0x1010ca000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.81+0x10>
10084432c:      add x0, x0, #0x1b8
100844330:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100844334:      adrp    x0, 0x1010c9000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1798+0x220>
100844338:      add x0, x0, #0xdf0
10084433c:      mov x1, sp
100844340:      bl  0x100135b88 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100844344:      mov x0, x19
100844348:      ldp x29, x30, [sp, #0x80]
10084434c:      ldp x20, x19, [sp, #0x70]
100844350:      ldp x22, x21, [sp, #0x60]
100844354:      ldp x24, x23, [sp, #0x50]
100844358:      ldp x26, x25, [sp, #0x40]
10084435c:      ldp x28, x27, [sp, #0x30]
100844360:      add sp, sp, #0x90
100844364:      ret
100844368:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10084436c:      ldr x26, [x0, #0x8]
100844370:      ldr x8, [x23]
100844374:      cmn x8, #0x1
100844378:      b.ne    0x1008440a8 <_js_array_grow+0x628>
10084437c:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100844380:      ldr x19, [x0]
100844384:      ldr x8, [x26]
100844388:      cbnz    x8, 0x1008440c4 <_js_array_grow+0x644>
10084438c:      b   0x1008440e4 <_js_array_grow+0x664>
100844390:      str x21, [sp, #0x10]
100844394:      add x8, sp, #0x10
100844398:      adrp    x9, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
10084439c:      add x9, x9, #0x288
1008443a0:      stp x8, x9, [sp, #0x18]
1008443a4:      adrp    x0, 0x100e17000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.1076+0xdb>
1008443a8:      add x0, x0, #0x176
1008443ac:      adrp    x2, 0x1010cc000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.856+0xc8>
1008443b0:      add x2, x2, #0xa60
1008443b4:      add x1, sp, #0x18
1008443b8:      bl  0x100c99d7c <__RNvNtCsjgY6bXVaRmE_4core9panicking9panic_fmt>
1008443bc:      mov x19, x1
1008443c0:      bl  0x100cb9d4c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008443c4:      mov x1, x19
1008443c8:      b   0x10084415c <_js_array_grow+0x6dc>
1008443cc:      adrp    x0, 0x1010c9000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1798+0x220>
1008443d0:      add x0, x0, #0xeb8
1008443d4:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008443d8:      adrp    x0, 0x1010c9000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1798+0x220>
1008443dc:      add x0, x0, #0xe58
1008443e0:      bl  0x100c99adc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008443e4:      bl  0x100cba99c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1008443e8:      adrp    x0, 0x100e14000 <_anon.fe8f47ec8f190e8b099ceccbb103e4e2.1999+0x14c>
1008443ec:      add x0, x0, #0x1cd
1008443f0:      mov w1, #0xb                ; =11
1008443f4:      bl  0x100cba964 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008443f8:      adrp    x2, 0x1010ca000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.81+0x10>
1008443fc:      add x2, x2, #0x7b0
100844400:      bl  0x100c99c0c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
100844404:      adrp    x2, 0x1010ca000 <_anon.78a33a9fe279ced61d81da3c9b3c7fad.81+0x10>
100844408:      add x2, x2, #0x7c8
10084440c:      mov x1, x8
100844410:      bl  0x100c99c0c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
        ...
