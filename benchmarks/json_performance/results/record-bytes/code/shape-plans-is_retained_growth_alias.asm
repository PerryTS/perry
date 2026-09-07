/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007d7e34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>:
1007d7e34:      stp x24, x23, [sp, #-0x40]!
1007d7e38:      stp x22, x21, [sp, #0x10]
1007d7e3c:      stp x20, x19, [sp, #0x20]
1007d7e40:      stp x29, x30, [sp, #0x30]
1007d7e44:      add x29, sp, #0x30
1007d7e48:      mov x19, x1
1007d7e4c:      lsr x8, x0, #48
1007d7e50:      mov w9, #0x7ffd             ; =32765
1007d7e54:      and x10, x0, #0xffffffffffff
1007d7e58:      cmp w8, #0x0
1007d7e5c:      csel    x11, x8, x0, ne
1007d7e60:      cset    w12, ne
1007d7e64:      cmp x8, x9
1007d7e68:      csel    x20, x10, x11, eq
1007d7e6c:      csel    w8, wzr, w12, eq
1007d7e70:      lsr x10, x1, #48
1007d7e74:      cbz x10, 0x1007d7e84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x50>
1007d7e78:      cmp w10, w9
1007d7e7c:      b.ne    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d7e80:      and x19, x19, #0xffffffffffff
1007d7e84:      cmp x20, x19
1007d7e88:      csinc   w8, w8, wzr, ne
1007d7e8c:      tbz w8, #0x0, 0x1007d7ea8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x74>
1007d7e90:      mov w0, #0x0                ; =0
1007d7e94:      ldp x29, x30, [sp, #0x30]
1007d7e98:      ldp x20, x19, [sp, #0x20]
1007d7e9c:      ldp x22, x21, [sp, #0x10]
1007d7ea0:      ldp x24, x23, [sp], #0x40
1007d7ea4:      ret
1007d7ea8:      mov w8, #0x3f               ; =63
1007d7eac:      adrp    x22, 0x101130000 <_perry_global_baseline_worker_ts__1>
1007d7eb0:      add x22, x22, #0x4e8
1007d7eb4:      mov x23, x8
1007d7eb8:      mov x0, x20
1007d7ebc:      bl  0x1007da4dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1007d7ec0:      mov x21, x0
1007d7ec4:      mov w0, #0x0                ; =0
1007d7ec8:      cbz x20, 0x1007d7e94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1007d7ecc:      cbz x21, 0x1007d7e94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1007d7ed0:      ldr x8, [x22]
1007d7ed4:      cmn x8, #0x1
1007d7ed8:      b.eq    0x1007d8014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
1007d7edc:      mrs x9, TPIDRRO_EL0
1007d7ee0:      and x9, x9, #0xfffffffffffffff8
1007d7ee4:      ldr x0, [x9, x8, lsl #3]
1007d7ee8:      cbz x0, 0x1007d8014 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
1007d7eec:      lsr x1, x20, #20
1007d7ef0:      ldr x8, [x0, #0x10]
1007d7ef4:      ldrb    w9, [x8, #0x28]
1007d7ef8:      tbz w9, #0x0, 0x1007d7f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
1007d7efc:      ldr x9, [x8, #0x20]
1007d7f00:      cmp x9, x1
1007d7f04:      b.ne    0x1007d7f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
1007d7f08:      ldp x9, x10, [x8]
1007d7f0c:      cmp x20, x9
1007d7f10:      ccmp    x20, x10, #0x2, hs
1007d7f14:      b.lo    0x1007d7f84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x150>
1007d7f18:      ldrb    w9, [x8, #0x58]
1007d7f1c:      cbz w9, 0x1007d7f3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
1007d7f20:      ldr x9, [x8, #0x50]
1007d7f24:      cmp x9, x1
1007d7f28:      b.ne    0x1007d7f3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
1007d7f2c:      ldp x9, x10, [x8, #0x30]
1007d7f30:      cmp x20, x9
1007d7f34:      ccmp    x20, x10, #0x2, hs
1007d7f38:      b.lo    0x1007d7fec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1b8>
1007d7f3c:      ldrb    w9, [x8, #0x88]
1007d7f40:      cbz w9, 0x1007d7f60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
1007d7f44:      ldr x9, [x8, #0x80]
1007d7f48:      cmp x9, x1
1007d7f4c:      b.ne    0x1007d7f60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
1007d7f50:      ldp x9, x10, [x8, #0x60]
1007d7f54:      cmp x20, x9
1007d7f58:      ccmp    x20, x10, #0x2, hs
1007d7f5c:      b.lo    0x1007d8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1cc>
1007d7f60:      ldrb    w9, [x8, #0xb8]
1007d7f64:      cbz w9, 0x1007d7f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1007d7f68:      ldr x9, [x8, #0xb0]
1007d7f6c:      cmp x9, x1
1007d7f70:      b.ne    0x1007d7f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1007d7f74:      ldp x9, x10, [x8, #0x90]!
1007d7f78:      cmp x20, x9
1007d7f7c:      ccmp    x20, x10, #0x2, hs
1007d7f80:      b.hs    0x1007d7f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1007d7f84:      ldrb    w8, [x8, #0x19]
1007d7f88:      cmp w8, #0xff
1007d7f8c:      b.ne    0x1007d7f9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
1007d7f90:      mov x0, x20
1007d7f94:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1007d7f98:      and w8, w0, #0xff
1007d7f9c:      and w8, w8, #0xfe
1007d7fa0:      cmp w8, #0x2
1007d7fa4:      b.ne    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d7fa8:      ldrb    w8, [x21]
1007d7fac:      cmp w8, #0x1
1007d7fb0:      b.ne    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d7fb4:      ldrsb   w8, [x21, #0x1]
1007d7fb8:      tbz w8, #0x1f, 0x1007d802c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1f8>
1007d7fbc:      ldrh    w8, [x21, #0x2]
1007d7fc0:      lsr w8, w8, #14
1007d7fc4:      cmp w8, #0x2
1007d7fc8:      b.ls    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d7fcc:      mov w0, #0x0                ; =0
1007d7fd0:      ldr x9, [x21, #0x8]
1007d7fd4:      cmp x20, x9
1007d7fd8:      b.eq    0x1007d7e94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1007d7fdc:      sub w8, w23, #0x1
1007d7fe0:      mov x20, x9
1007d7fe4:      cbnz    w23, 0x1007d7eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x80>
1007d7fe8:      b   0x1007d7e94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1007d7fec:      add x8, x8, #0x30
1007d7ff0:      ldrb    w8, [x8, #0x19]
1007d7ff4:      cmp w8, #0xff
1007d7ff8:      b.ne    0x1007d7f9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
1007d7ffc:      b   0x1007d7f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1007d8000:      add x8, x8, #0x60
1007d8004:      ldrb    w8, [x8, #0x19]
1007d8008:      cmp w8, #0xff
1007d800c:      b.ne    0x1007d7f9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
1007d8010:      b   0x1007d7f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1007d8014:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1007d8018:      lsr x1, x20, #20
1007d801c:      ldr x8, [x0, #0x10]
1007d8020:      ldrb    w9, [x8, #0x28]
1007d8024:      tbnz    w9, #0x0, 0x1007d7efc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xc8>
1007d8028:      b   0x1007d7f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
1007d802c:      cmp x20, x19
1007d8030:      b.ne    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d8034:      ldrh    w8, [x21, #0x2]
1007d8038:      lsr w8, w8, #14
1007d803c:      cmp w8, #0x2
1007d8040:      b.hi    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d8044:      ldp w9, w8, [x20]
1007d8048:      cmp w9, w8
1007d804c:      b.hi    0x1007d7e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1007d8050:      ldr w9, [x21, #0x4]
1007d8054:      subs    x9, x9, #0x10
1007d8058:      csel    x9, xzr, x9, lo
1007d805c:      cmp x8, x9, lsr #3
1007d8060:      cset    w0, ls
1007d8064:      b   0x1007d7e94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
