/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100548f34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>:
100548f34:      stp x24, x23, [sp, #-0x40]!
100548f38:      stp x22, x21, [sp, #0x10]
100548f3c:      stp x20, x19, [sp, #0x20]
100548f40:      stp x29, x30, [sp, #0x30]
100548f44:      add x29, sp, #0x30
100548f48:      mov x19, x1
100548f4c:      lsr x8, x0, #48
100548f50:      mov w9, #0x7ffd             ; =32765
100548f54:      and x10, x0, #0xffffffffffff
100548f58:      cmp w8, #0x0
100548f5c:      csel    x11, x8, x0, ne
100548f60:      cset    w12, ne
100548f64:      cmp x8, x9
100548f68:      csel    x20, x10, x11, eq
100548f6c:      csel    w8, wzr, w12, eq
100548f70:      lsr x10, x1, #48
100548f74:      cbz x10, 0x100548f84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x50>
100548f78:      cmp w10, w9
100548f7c:      b.ne    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100548f80:      and x19, x19, #0xffffffffffff
100548f84:      cmp x20, x19
100548f88:      csinc   w8, w8, wzr, ne
100548f8c:      tbz w8, #0x0, 0x100548fa8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x74>
100548f90:      mov w0, #0x0                ; =0
100548f94:      ldp x29, x30, [sp, #0x30]
100548f98:      ldp x20, x19, [sp, #0x20]
100548f9c:      ldp x22, x21, [sp, #0x10]
100548fa0:      ldp x24, x23, [sp], #0x40
100548fa4:      ret
100548fa8:      mov w8, #0x3f               ; =63
100548fac:      adrp    x22, 0x101130000 <_perry_global_baseline_worker_ts__1>
100548fb0:      add x22, x22, #0x360
100548fb4:      mov x23, x8
100548fb8:      mov x0, x20
100548fbc:      bl  0x10054b5dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100548fc0:      mov x21, x0
100548fc4:      mov w0, #0x0                ; =0
100548fc8:      cbz x20, 0x100548f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100548fcc:      cbz x21, 0x100548f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100548fd0:      ldr x8, [x22]
100548fd4:      cmn x8, #0x1
100548fd8:      b.eq    0x100549114 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
100548fdc:      mrs x9, TPIDRRO_EL0
100548fe0:      and x9, x9, #0xfffffffffffffff8
100548fe4:      ldr x0, [x9, x8, lsl #3]
100548fe8:      cbz x0, 0x100549114 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
100548fec:      lsr x1, x20, #20
100548ff0:      ldr x8, [x0, #0x10]
100548ff4:      ldrb    w9, [x8, #0x28]
100548ff8:      tbz w9, #0x0, 0x100549018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
100548ffc:      ldr x9, [x8, #0x20]
100549000:      cmp x9, x1
100549004:      b.ne    0x100549018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
100549008:      ldp x9, x10, [x8]
10054900c:      cmp x20, x9
100549010:      ccmp    x20, x10, #0x2, hs
100549014:      b.lo    0x100549084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x150>
100549018:      ldrb    w9, [x8, #0x58]
10054901c:      cbz w9, 0x10054903c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
100549020:      ldr x9, [x8, #0x50]
100549024:      cmp x9, x1
100549028:      b.ne    0x10054903c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
10054902c:      ldp x9, x10, [x8, #0x30]
100549030:      cmp x20, x9
100549034:      ccmp    x20, x10, #0x2, hs
100549038:      b.lo    0x1005490ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1b8>
10054903c:      ldrb    w9, [x8, #0x88]
100549040:      cbz w9, 0x100549060 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
100549044:      ldr x9, [x8, #0x80]
100549048:      cmp x9, x1
10054904c:      b.ne    0x100549060 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
100549050:      ldp x9, x10, [x8, #0x60]
100549054:      cmp x20, x9
100549058:      ccmp    x20, x10, #0x2, hs
10054905c:      b.lo    0x100549100 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1cc>
100549060:      ldrb    w9, [x8, #0xb8]
100549064:      cbz w9, 0x100549090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100549068:      ldr x9, [x8, #0xb0]
10054906c:      cmp x9, x1
100549070:      b.ne    0x100549090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100549074:      ldp x9, x10, [x8, #0x90]!
100549078:      cmp x20, x9
10054907c:      ccmp    x20, x10, #0x2, hs
100549080:      b.hs    0x100549090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100549084:      ldrb    w8, [x8, #0x19]
100549088:      cmp w8, #0xff
10054908c:      b.ne    0x10054909c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
100549090:      mov x0, x20
100549094:      bl  0x1002bbee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100549098:      and w8, w0, #0xff
10054909c:      and w8, w8, #0xfe
1005490a0:      cmp w8, #0x2
1005490a4:      b.ne    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005490a8:      ldrb    w8, [x21]
1005490ac:      cmp w8, #0x1
1005490b0:      b.ne    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005490b4:      ldrsb   w8, [x21, #0x1]
1005490b8:      tbz w8, #0x1f, 0x10054912c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1f8>
1005490bc:      ldrh    w8, [x21, #0x2]
1005490c0:      lsr w8, w8, #14
1005490c4:      cmp w8, #0x2
1005490c8:      b.ls    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005490cc:      mov w0, #0x0                ; =0
1005490d0:      ldr x9, [x21, #0x8]
1005490d4:      cmp x20, x9
1005490d8:      b.eq    0x100548f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1005490dc:      sub w8, w23, #0x1
1005490e0:      mov x20, x9
1005490e4:      cbnz    w23, 0x100548fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x80>
1005490e8:      b   0x100548f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
1005490ec:      add x8, x8, #0x30
1005490f0:      ldrb    w8, [x8, #0x19]
1005490f4:      cmp w8, #0xff
1005490f8:      b.ne    0x10054909c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
1005490fc:      b   0x100549090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100549100:      add x8, x8, #0x60
100549104:      ldrb    w8, [x8, #0x19]
100549108:      cmp w8, #0xff
10054910c:      b.ne    0x10054909c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
100549110:      b   0x100549090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100549114:      bl  0x100caf8ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100549118:      lsr x1, x20, #20
10054911c:      ldr x8, [x0, #0x10]
100549120:      ldrb    w9, [x8, #0x28]
100549124:      tbnz    w9, #0x0, 0x100548ffc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xc8>
100549128:      b   0x100549018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
10054912c:      cmp x20, x19
100549130:      b.ne    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100549134:      ldrh    w8, [x21, #0x2]
100549138:      lsr w8, w8, #14
10054913c:      cmp w8, #0x2
100549140:      b.hi    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100549144:      ldp w9, w8, [x20]
100549148:      cmp w9, w8
10054914c:      b.hi    0x100548f90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100549150:      ldr w9, [x21, #0x4]
100549154:      subs    x9, x9, #0x10
100549158:      csel    x9, xzr, x9, lo
10054915c:      cmp x8, x9, lsr #3
100549160:      cset    w0, ls
100549164:      b   0x100548f94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
