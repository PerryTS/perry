/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100395a5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array>:
100395a5c:      stp x26, x25, [sp, #-0x50]!
100395a60:      stp x24, x23, [sp, #0x10]
100395a64:      stp x22, x21, [sp, #0x20]
100395a68:      stp x20, x19, [sp, #0x30]
100395a6c:      stp x29, x30, [sp, #0x40]
100395a70:      add x29, sp, #0x40
100395a74:      mov x19, x0
100395a78:      ldr x23, [x19, #0x20]!
100395a7c:      cbz x23, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395a80:      lsr x8, x23, #51
100395a84:      mov x21, x23
100395a88:      cmp x8, #0xfff
100395a8c:      b.lo    0x100395aa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x48>
100395a90:      mov w8, #0x7ffc             ; =32764
100395a94:      cmp x8, x23, lsr #48
100395a98:      b.eq    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395a9c:      ands    x21, x23, #0xffffffffffff
100395aa0:      b.eq    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395aa4:      and x8, x21, #0xfffffffffff00000
100395aa8:      lsr x9, x21, #47
100395aac:      cmp x9, #0x0
100395ab0:      ccmp    x8, #0x0, #0x4, eq
100395ab4:      b.eq    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395ab8:      tst x21, #0x3
100395abc:      ccmp    x21, #0x7, #0x0, eq
100395ac0:      mov x20, x0
100395ac4:      b.ls    0x100395bd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100395ac8:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
100395acc:      add x8, x8, #0x8f0
100395ad0:      ldr x8, [x8]
100395ad4:      cmn x8, #0x1
100395ad8:      b.eq    0x100395ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
100395adc:      mrs x9, TPIDRRO_EL0
100395ae0:      and x9, x9, #0xfffffffffffffff8
100395ae4:      ldr x8, [x9, x8, lsl #3]
100395ae8:      cbz x8, 0x100395ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
100395aec:      lsr x1, x21, #20
100395af0:      ldr x8, [x8, #0x10]
100395af4:      ldrb    w9, [x8, #0x28]
100395af8:      tbz w9, #0x0, 0x100395b18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100395afc:      ldr x9, [x8, #0x20]
100395b00:      cmp x9, x1
100395b04:      b.ne    0x100395b18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100395b08:      ldp x9, x10, [x8]
100395b0c:      cmp x9, x21
100395b10:      ccmp    x10, x21, #0x0, ls
100395b14:      b.hi    0x100395b94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
100395b18:      ldrb    w9, [x8, #0x58]
100395b1c:      cbz w9, 0x100395b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
100395b20:      ldr x9, [x8, #0x50]
100395b24:      cmp x9, x1
100395b28:      b.ne    0x100395b3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
100395b2c:      ldp x9, x10, [x8, #0x30]
100395b30:      cmp x9, x21
100395b34:      ccmp    x10, x21, #0x0, ls
100395b38:      b.hi    0x100395b88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x12c>
100395b3c:      ldrb    w9, [x8, #0x88]
100395b40:      cbz w9, 0x100395b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
100395b44:      ldr x9, [x8, #0x80]
100395b48:      cmp x9, x1
100395b4c:      b.ne    0x100395b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
100395b50:      ldp x9, x10, [x8, #0x60]
100395b54:      cmp x9, x21
100395b58:      ccmp    x10, x21, #0x0, ls
100395b5c:      b.hi    0x100395b90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x134>
100395b60:      ldrb    w9, [x8, #0xb8]
100395b64:      cbz w9, 0x100395ba0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
100395b68:      ldr x9, [x8, #0xb0]
100395b6c:      cmp x9, x1
100395b70:      b.ne    0x100395ba0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
100395b74:      ldp x9, x10, [x8, #0x90]!
100395b78:      cmp x9, x21
100395b7c:      ccmp    x10, x21, #0x0, ls
100395b80:      b.hi    0x100395b94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
100395b84:      b   0x100395ba0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
100395b88:      add x8, x8, #0x30
100395b8c:      b   0x100395b94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
100395b90:      add x8, x8, #0x60
100395b94:      ldrb    w8, [x8, #0x19]
100395b98:      cmp w8, #0xff
100395b9c:      b.ne    0x100395bb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x158>
100395ba0:      mov x0, x21
100395ba4:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100395ba8:      mov x8, x0
100395bac:      mov x0, x20
100395bb0:      and w8, w8, #0xff
100395bb4:      cbz w8, 0x100395bd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100395bb8:      ldurb   w8, [x21, #-0x8]
100395bbc:      ldurb   w9, [x21, #-0x7]
100395bc0:      mov w10, #0x82              ; =130
100395bc4:      and w9, w9, w10
100395bc8:      cmp w9, #0x2
100395bcc:      ccmp    w8, #0x1, #0x0, eq
100395bd0:      b.eq    0x100395cf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x298>
100395bd4:      mov x0, x21
100395bd8:      bl  0x10037fd1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100395bdc:      cbz x0, 0x100395c0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x1b0>
100395be0:      ldrb    w9, [x0]
100395be4:      cmp w9, #0x1
100395be8:      b.ne    0x100395cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x254>
100395bec:      ldrsb   w8, [x0, #0x1]
100395bf0:      tbnz    w8, #0x1f, 0x100395d20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2c4>
100395bf4:      mov x8, x0
100395bf8:      mov x0, x20
100395bfc:      ldp w10, w9, [x21]
100395c00:      cmp w10, w9
100395c04:      b.hi    0x100395db8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x35c>
100395c08:      b   0x100395dd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
100395c0c:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
100395c10:      add x8, x8, #0xf2a
100395c14:      ldaprb  w8, [x8]
100395c18:      cbz w8, 0x100395c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100395c1c:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
100395c20:      add x8, x8, #0xbb0
100395c24:      ldapr   x9, [x8]
100395c28:      cmp x9, x21
100395c2c:      b.hi    0x100395c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100395c30:      ldapur  x8, [x8, #0x8]
100395c34:      cmp x8, x21
100395c38:      b.lo    0x100395c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100395c3c:      mov x24, x0
100395c40:      mov x0, x21
100395c44:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100395c48:      mov x8, x0
100395c4c:      mov x0, x24
100395c50:      tbz w8, #0x0, 0x100395c5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
100395c54:      mov x8, #0x0                ; =0
100395c58:      b   0x100395da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
100395c5c:      adrp    x8, 0x101211000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfc20>
100395c60:      add x8, x8, #0xad0
100395c64:      ldaprb  w8, [x8]
100395c68:      cbz w8, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395c6c:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395c70:      add x8, x8, #0x8c0
100395c74:      ldapr   x9, [x8]
100395c78:      cmp x9, x21
100395c7c:      b.hi    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395c80:      ldapur  x8, [x8, #0x8]
100395c84:      cmp x8, x21
100395c88:      b.lo    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395c8c:      mov x24, x0
100395c90:      mov x0, x21
100395c94:      bl  0x1008bdee8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100395c98:      mov x9, x0
100395c9c:      mov x8, #0x0                ; =0
100395ca0:      mov x22, #0x0               ; =0
100395ca4:      mov x0, x20
100395ca8:      tbnz    w9, #0x0, 0x100395da8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x34c>
100395cac:      b   0x100395ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
100395cb0:      mov x24, x0
100395cb4:      mov x8, x0
100395cb8:      cmp w9, #0x1
100395cbc:      b.eq    0x100395da4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
100395cc0:      cmp w9, #0x9
100395cc4:      b.ne    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395cc8:      ldr w8, [x21, #0x4]
100395ccc:      mov w9, #0x5841             ; =22593
100395cd0:      movk    w9, #0x4c5a, lsl #16
100395cd4:      cmp w8, w9
100395cd8:      b.ne    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395cdc:      mov x0, x21
100395ce0:      bl  0x10035a198 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
100395ce4:      mov x22, x0
100395ce8:      mov x0, x20
100395cec:      cbnz    x22, 0x100395e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100395cf0:      b   0x100395ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
100395cf4:      ldr w8, [x21]
100395cf8:      mov w9, #0xe100             ; =57600
100395cfc:      movk    w9, #0x5f5, lsl #16
100395d00:      orr w9, w9, #0x1
100395d04:      cmp w8, w9
100395d08:      b.hs    0x100395bd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100395d0c:      ldr w9, [x21, #0x4]
100395d10:      cmp w8, w9
100395d14:      b.hi    0x100395bd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
100395d18:      mov x22, x21
100395d1c:      b   0x100395e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100395d20:      mov x24, x0
100395d24:      ldr x21, [x0, #0x8]
100395d28:      mov x0, x21
100395d2c:      bl  0x10037fd1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100395d30:      cbz x0, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395d34:      mov x8, x0
100395d38:      ldrb    w9, [x0]
100395d3c:      cmp w9, #0x1
100395d40:      b.ne    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395d44:      ldrsb   w9, [x8, #0x1]
100395d48:      tbz w9, #0x1f, 0x100395bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x19c>
100395d4c:      mov w25, #0x1               ; =1
100395d50:      ldr x21, [x8, #0x8]
100395d54:      mov x0, x21
100395d58:      bl  0x10037fd1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100395d5c:      cbz x0, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395d60:      mov x8, x0
100395d64:      mov x22, #0x0               ; =0
100395d68:      ldrb    w9, [x0]
100395d6c:      cmp w9, #0x1
100395d70:      b.ne    0x100395ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
100395d74:      cmp w25, #0x3f
100395d78:      b.hi    0x100395ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
100395d7c:      add w25, w25, #0x1
100395d80:      ldrsb   w9, [x8, #0x1]
100395d84:      tbnz    w9, #0x1f, 0x100395d50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2f4>
100395d88:      str x21, [x24, #0x8]
100395d8c:      ldrb    w10, [x24, #0x1]
100395d90:      orr w10, w10, #0x80
100395d94:      strb    w10, [x24, #0x1]
100395d98:      ldrb    w9, [x8]
100395d9c:      cmp w9, #0x1
100395da0:      b.ne    0x100395cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x264>
100395da4:      mov x0, x20
100395da8:      ldp w10, w9, [x21]
100395dac:      cmp w10, w9
100395db0:      b.ls    0x100395dd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
100395db4:      cbz x24, 0x100395de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
100395db8:      ldr w8, [x8, #0x4]
100395dbc:      ubfiz   x9, x9, #3, #32
100395dc0:      add x9, x9, #0x10
100395dc4:      mov x22, x21
100395dc8:      cmp x9, x8
100395dcc:      b.eq    0x100395e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100395dd0:      b   0x100395de8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
100395dd4:      mov w8, #0xe100             ; =57600
100395dd8:      movk    w8, #0x5f5, lsl #16
100395ddc:      mov x22, x21
100395de0:      cmp w10, w8
100395de4:      b.ls    0x100395e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100395de8:      adrp    x8, 0x101200000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7a8>
100395dec:      add x8, x8, #0xf2a
100395df0:      ldaprb  w8, [x8]
100395df4:      cbz w8, 0x100395e30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100395df8:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
100395dfc:      add x8, x8, #0xbb0
100395e00:      ldapr   x9, [x8]
100395e04:      cmp x9, x21
100395e08:      b.hi    0x100395e30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100395e0c:      ldapur  x8, [x8, #0x8]
100395e10:      cmp x8, x21
100395e14:      b.lo    0x100395e30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100395e18:      mov x0, x21
100395e1c:      bl  0x1004c8a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100395e20:      tbz w0, #0x0, 0x100395e30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
100395e24:      mov x0, x20
100395e28:      mov x22, x21
100395e2c:      b   0x100395e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
100395e30:      adrp    x8, 0x101211000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfc20>
100395e34:      add x8, x8, #0xad0
100395e38:      ldaprb  w8, [x8]
100395e3c:      cbz w8, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395e40:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395e44:      add x8, x8, #0x8c0
100395e48:      ldapr   x9, [x8]
100395e4c:      cmp x21, x9
100395e50:      b.lo    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395e54:      ldapur  x8, [x8, #0x8]
100395e58:      cmp x21, x8
100395e5c:      b.hi    0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395e60:      mov x0, x21
100395e64:      bl  0x1008bdee8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100395e68:      mov x8, x0
100395e6c:      mov x0, x20
100395e70:      mov x22, x21
100395e74:      tbz w8, #0x0, 0x100395eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
100395e78:      cmp x22, x23
100395e7c:      b.eq    0x100395f54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
100395e80:      str x22, [x0, #0x20]
100395e84:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395e88:      add x8, x8, #0x1b8
100395e8c:      ldapr   x8, [x8]
100395e90:      cbnz    x8, 0x100395ef8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x49c>
100395e94:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395e98:      ldrb    w8, [x8, #0x1c0]
100395e9c:      tbz w8, #0x0, 0x100395f14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4b8>
100395ea0:      mov x0, x20
100395ea4:      mov x1, x19
100395ea8:      mov x2, x22
100395eac:      mov w3, #0x0                ; =0
100395eb0:      bl  0x100680e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
100395eb4:      b   0x100395f2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d0>
100395eb8:      mov x22, #0x0               ; =0
100395ebc:      mov x0, x22
100395ec0:      ldp x29, x30, [sp, #0x40]
100395ec4:      ldp x20, x19, [sp, #0x30]
100395ec8:      ldp x22, x21, [sp, #0x20]
100395ecc:      ldp x24, x23, [sp, #0x10]
100395ed0:      ldp x26, x25, [sp], #0x50
100395ed4:      ret
100395ed8:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100395edc:      mov x8, x0
100395ee0:      mov x0, x20
100395ee4:      lsr x1, x21, #20
100395ee8:      ldr x8, [x8, #0x10]
100395eec:      ldrb    w9, [x8, #0x28]
100395ef0:      tbnz    w9, #0x0, 0x100395afc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xa0>
100395ef4:      b   0x100395b18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
100395ef8:      adrp    x0, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395efc:      add x0, x0, #0x1b8
100395f00:      bl  0x100cc433c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
100395f04:      mov x0, x20
100395f08:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
100395f0c:      ldrb    w8, [x8, #0x1c0]
100395f10:      tbnz    w8, #0x0, 0x100395ea0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x444>
100395f14:      adrp    x8, 0x101201000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc11instruments24INCREMENTAL_CYCLE_STARTS>
100395f18:      add x8, x8, #0x184
100395f1c:      ldr w8, [x8]
100395f20:      cbz w8, 0x100395f30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d4>
100395f24:      mov x0, x22
100395f28:      bl  0x100682074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100395f2c:      mov x0, x20
100395f30:      ldr x8, [x19]
100395f34:      cbz x8, 0x100395f54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
100395f38:      ldr x8, [x0, #0x10]
100395f3c:      cbz x8, 0x100395f54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
100395f40:      mov x0, x20
100395f44:      bl  0x100595b64 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime15json_tape_store7release>
100395f48:      mov x0, x20
100395f4c:      str xzr, [x20, #0x10]
100395f50:      str wzr, [x20, #0xc]
100395f54:      ldr w8, [x22]
100395f58:      str w8, [x0]
100395f5c:      b   0x100395ebc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
