/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100221dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>:
100221dc8:      sub sp, sp, #0xf0
100221dcc:      stp d9, d8, [sp, #0x80]
100221dd0:      stp x28, x27, [sp, #0x90]
100221dd4:      stp x26, x25, [sp, #0xa0]
100221dd8:      stp x24, x23, [sp, #0xb0]
100221ddc:      stp x22, x21, [sp, #0xc0]
100221de0:      stp x20, x19, [sp, #0xd0]
100221de4:      stp x29, x30, [sp, #0xe0]
100221de8:      add x29, sp, #0xe0
100221dec:      cmp w2, #0x3e9
100221df0:      b.hs    0x100223d78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1fb0>
100221df4:      mov x20, x2
100221df8:      mov x19, x1
100221dfc:      mov x22, x0
100221e00:      lsr x8, x0, #51
100221e04:      cmp x8, #0xfff
100221e08:      b.lo    0x100221e20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x58>
100221e0c:      mov w8, #0x7ffc             ; =32764
100221e10:      cmp x8, x22, lsr #48
100221e14:      b.eq    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221e18:      ands    x22, x22, #0xffffffffffff
100221e1c:      b.eq    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221e20:      and x8, x22, #0xfffffffffff00000
100221e24:      lsr x9, x22, #47
100221e28:      cmp x9, #0x0
100221e2c:      ccmp    x8, #0x0, #0x4, eq
100221e30:      b.eq    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221e34:      tst x22, #0x3
100221e38:      ccmp    x22, #0x7, #0x0, eq
100221e3c:      b.ls    0x100221f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100221e40:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100221e44:      add x8, x8, #0xfe0
100221e48:      ldr x8, [x8]
100221e4c:      cmn x8, #0x1
100221e50:      b.eq    0x100222bd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe08>
100221e54:      mrs x9, TPIDRRO_EL0
100221e58:      and x9, x9, #0xfffffffffffffff8
100221e5c:      ldr x0, [x9, x8, lsl #3]
100221e60:      cbz x0, 0x100222bd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe08>
100221e64:      lsr x1, x22, #20
100221e68:      ldr x8, [x0, #0x10]
100221e6c:      ldrb    w9, [x8, #0x28]
100221e70:      tbz w9, #0x0, 0x100221e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100221e74:      ldr x9, [x8, #0x20]
100221e78:      cmp x9, x1
100221e7c:      b.ne    0x100221e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100221e80:      ldp x9, x10, [x8]
100221e84:      cmp x9, x22
100221e88:      ccmp    x10, x22, #0x0, ls
100221e8c:      b.hi    0x100221f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100221e90:      ldrb    w9, [x8, #0x58]
100221e94:      cbz w9, 0x100221eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
100221e98:      ldr x9, [x8, #0x50]
100221e9c:      cmp x9, x1
100221ea0:      b.ne    0x100221eb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec>
100221ea4:      ldp x9, x10, [x8, #0x30]
100221ea8:      cmp x9, x22
100221eac:      ccmp    x10, x22, #0x0, ls
100221eb0:      b.hi    0x100221f00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x138>
100221eb4:      ldrb    w9, [x8, #0x88]
100221eb8:      cbz w9, 0x100221ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
100221ebc:      ldr x9, [x8, #0x80]
100221ec0:      cmp x9, x1
100221ec4:      b.ne    0x100221ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x110>
100221ec8:      ldp x9, x10, [x8, #0x60]
100221ecc:      cmp x9, x22
100221ed0:      ccmp    x10, x22, #0x0, ls
100221ed4:      b.hi    0x100221f08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x140>
100221ed8:      ldrb    w9, [x8, #0xb8]
100221edc:      cbz w9, 0x100221f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100221ee0:      ldr x9, [x8, #0xb0]
100221ee4:      cmp x9, x1
100221ee8:      b.ne    0x100221f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100221eec:      ldp x9, x10, [x8, #0x90]!
100221ef0:      cmp x9, x22
100221ef4:      ccmp    x10, x22, #0x0, ls
100221ef8:      b.hi    0x100221f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100221efc:      b   0x100221f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x150>
100221f00:      add x8, x8, #0x30
100221f04:      b   0x100221f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x144>
100221f08:      add x8, x8, #0x60
100221f0c:      ldrb    w8, [x8, #0x19]
100221f10:      cmp w8, #0xff
100221f14:      b.ne    0x100221f24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15c>
100221f18:      mov x0, x22
100221f1c:      bl  0x1009960b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
100221f20:      and w8, w0, #0xff
100221f24:      cbz w8, 0x100221f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
100221f28:      ldurb   w8, [x22, #-0x8]
100221f2c:      ldurb   w9, [x22, #-0x7]
100221f30:      mov w10, #0x82              ; =130
100221f34:      and w9, w9, w10
100221f38:      cmp w9, #0x2
100221f3c:      ccmp    w8, #0x1, #0x0, eq
100221f40:      b.eq    0x1002221ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3e4>
100221f44:      mov x0, x22
100221f48:      bl  0x10022ae58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100221f4c:      mov x8, x0
100221f50:      cbz x0, 0x100221fe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x21c>
100221f54:      ldrb    w9, [x8]
100221f58:      cmp w9, #0x1
100221f5c:      b.ne    0x10022206c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a4>
100221f60:      ldrsb   w9, [x8, #0x1]
100221f64:      mov x0, x8
100221f68:      tbz w9, #0x1f, 0x1002220ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2e4>
100221f6c:      mov x21, x8
100221f70:      ldr x22, [x8, #0x8]
100221f74:      mov x0, x22
100221f78:      bl  0x10022ae58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100221f7c:      cbz x0, 0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221f80:      ldrb    w8, [x0]
100221f84:      cmp w8, #0x1
100221f88:      b.ne    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221f8c:      ldrsb   w8, [x0, #0x1]
100221f90:      tbz w8, #0x1f, 0x100222064 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x29c>
100221f94:      mov w23, #0x1               ; =1
100221f98:      ldr x22, [x0, #0x8]
100221f9c:      mov x0, x22
100221fa0:      bl  0x10022ae58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100221fa4:      cbz x0, 0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221fa8:      ldrb    w8, [x0]
100221fac:      cmp w8, #0x1
100221fb0:      b.ne    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221fb4:      cmp w23, #0x3f
100221fb8:      b.hi    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100221fbc:      add w23, w23, #0x1
100221fc0:      ldrsb   w8, [x0, #0x1]
100221fc4:      tbnz    w8, #0x1f, 0x100221f98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d0>
100221fc8:      mov x8, x21
100221fcc:      str x22, [x21, #0x8]
100221fd0:      ldrb    w9, [x21, #0x1]
100221fd4:      orr w9, w9, #0x80
100221fd8:      strb    w9, [x21, #0x1]
100221fdc:      ldrb    w9, [x0]
100221fe0:      b   0x100222070 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2a8>
100221fe4:      mov x21, x8
100221fe8:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
100221fec:      add x8, x8, #0x710
100221ff0:      ldaprb  w8, [x8]
100221ff4:      cbz w8, 0x100222024 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
100221ff8:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
100221ffc:      add x8, x8, #0x258
100222000:      ldapr   x9, [x8]
100222004:      cmp x9, x22
100222008:      b.hi    0x100222024 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
10022200c:      ldapur  x8, [x8, #0x8]
100222010:      cmp x8, x22
100222014:      b.lo    0x100222024 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x25c>
100222018:      mov x0, x22
10022201c:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100222020:      tbnz    w0, #0x0, 0x100222060 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x298>
100222024:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
100222028:      add x8, x8, #0xa61
10022202c:      ldaprb  w8, [x8]
100222030:      cbz w8, 0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222034:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100222038:      add x8, x8, #0xae8
10022203c:      ldapr   x9, [x8]
100222040:      cmp x9, x22
100222044:      b.hi    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222048:      ldapur  x8, [x8, #0x8]
10022204c:      cmp x8, x22
100222050:      b.lo    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222054:      mov x0, x22
100222058:      bl  0x100895b28 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
10022205c:      tbz w0, #0x0, 0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222060:      mov x0, #0x0                ; =0
100222064:      mov x8, x21
100222068:      b   0x1002220ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2e4>
10022206c:      mov x0, x8
100222070:      cmp w9, #0x1
100222074:      b.eq    0x1002220ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2e4>
100222078:      cmp w9, #0x9
10022207c:      b.ne    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222080:      ldr w8, [x22, #0x4]
100222084:      mov w9, #0x5841             ; =22593
100222088:      movk    w9, #0x4c5a, lsl #16
10022208c:      cmp w8, w9
100222090:      b.ne    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222094:      mov x0, x22
100222098:      bl  0x1003db41c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
10022209c:      mov x22, x0
1002220a0:      str x0, [sp, #0x18]
1002220a4:      cbnz    x0, 0x1002221d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x40c>
1002220a8:      b   0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
1002220ac:      ldp w10, w9, [x22]
1002220b0:      cmp w10, w9
1002220b4:      b.ls    0x1002220d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x30c>
1002220b8:      cbz x8, 0x1002220e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x31c>
1002220bc:      ldr w8, [x0, #0x4]
1002220c0:      lsl x9, x9, #3
1002220c4:      add x9, x9, #0x10
1002220c8:      cmp x9, x8
1002220cc:      b.ne    0x1002220e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x31c>
1002220d0:      b   0x1002221d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x408>
1002220d4:      mov w8, #0xe100             ; =57600
1002220d8:      movk    w8, #0x5f5, lsl #16
1002220dc:      cmp w10, w8
1002220e0:      b.ls    0x1002221d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x408>
1002220e4:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
1002220e8:      add x8, x8, #0x710
1002220ec:      ldaprb  w8, [x8]
1002220f0:      cbz w8, 0x100222120 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x358>
1002220f4:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1002220f8:      add x8, x8, #0x258
1002220fc:      ldapr   x9, [x8]
100222100:      cmp x9, x22
100222104:      b.hi    0x100222120 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x358>
100222108:      ldapur  x8, [x8, #0x8]
10022210c:      cmp x8, x22
100222110:      b.lo    0x100222120 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x358>
100222114:      mov x0, x22
100222118:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
10022211c:      tbnz    w0, #0x0, 0x1002221d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x408>
100222120:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
100222124:      add x8, x8, #0xa61
100222128:      ldaprb  w8, [x8]
10022212c:      cbz w8, 0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222130:      adrp    x8, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100222134:      add x8, x8, #0xae8
100222138:      ldapr   x9, [x8]
10022213c:      cmp x9, x22
100222140:      b.hi    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222144:      ldapur  x8, [x8, #0x8]
100222148:      cmp x8, x22
10022214c:      b.lo    0x10022215c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x394>
100222150:      mov x0, x22
100222154:      bl  0x100895b28 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
100222158:      tbnz    w0, #0x0, 0x1002221d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x408>
10022215c:      ldr x1, [x19, #0x10]
100222160:      ldr x8, [x19]
100222164:      sub x8, x8, x1
100222168:      cmp x8, #0x1
10022216c:      b.ls    0x100222ecc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1104>
100222170:      ldr x8, [x19, #0x8]
100222174:      mov w9, #0x5d5b             ; =23899
100222178:      strh    w9, [x8, x1]
10022217c:      ldr x8, [x19, #0x10]
100222180:      add x8, x8, #0x2
100222184:      str x8, [x19, #0x10]
100222188:      ldp x29, x30, [sp, #0xe0]
10022218c:      ldp x20, x19, [sp, #0xd0]
100222190:      ldp x22, x21, [sp, #0xc0]
100222194:      ldp x24, x23, [sp, #0xb0]
100222198:      ldp x26, x25, [sp, #0xa0]
10022219c:      ldp x28, x27, [sp, #0x90]
1002221a0:      ldp d9, d8, [sp, #0x80]
1002221a4:      add sp, sp, #0xf0
1002221a8:      ret
1002221ac:      ldr w8, [x22]
1002221b0:      mov w9, #0xe100             ; =57600
1002221b4:      movk    w9, #0x5f5, lsl #16
1002221b8:      orr w9, w9, #0x1
1002221bc:      cmp w8, w9
1002221c0:      b.hs    0x100221f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1002221c4:      ldr w9, [x22, #0x4]
1002221c8:      cmp w8, w9
1002221cc:      b.hi    0x100221f44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17c>
1002221d0:      str x22, [sp, #0x18]
1002221d4:      mov x0, x22
1002221d8:      bl  0x100220c0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17array_get_to_json>
1002221dc:      tbz w0, #0x0, 0x100222274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4ac>
1002221e0:      fmov    x21, d0
1002221e4:      mov w8, #0x7ffd             ; =32765
1002221e8:      cmp x8, x21, lsr #48
1002221ec:      b.ne    0x100222b84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdbc>
1002221f0:      and x21, x21, #0xffffffffffff
1002221f4:      sub x8, x21, #0x100, lsl #12 ; =0x100000
1002221f8:      mov x9, #0x7ffffff00000     ; =140737487306752
1002221fc:      cmp x8, x9
100222200:      b.hs    0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222204:      ldurb   w8, [x21, #-0x8]
100222208:      sub w8, w8, #0x1
10022220c:      cmp w8, #0x1
100222210:      b.hi    0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222214:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
100222218:      add x8, x8, #0x710
10022221c:      ldaprb  w8, [x8]
100222220:      cbz w8, 0x100222258 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x490>
100222224:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
100222228:      add x8, x8, #0x258
10022222c:      ldapr   x9, [x8]
100222230:      cmp x21, x9
100222234:      b.lo    0x100222258 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x490>
100222238:      ldapur  x8, [x8, #0x8]
10022223c:      cmp x21, x8
100222240:      b.hi    0x100222258 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x490>
100222244:      mov x0, x21
100222248:      mov.16b v8, v0
10022224c:      bl  0x10019dd98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
100222250:      mov.16b v0, v8
100222254:      tbnz    w0, #0x0, 0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222258:      adrp    x0, 0x101125000 <__MergedGlobals+0xc0>
10022225c:      add x0, x0, #0xc18
100222260:      ldr x8, [x0]
100222264:      blr x8
100222268:      mov w8, #0x1                ; =1
10022226c:      strb    w8, [x0]
100222270:      b   0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222274:      mov x0, x22
100222278:      bl  0x10022ae58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10022227c:      cbz x0, 0x1002222ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4e4>
100222280:      ldrb    w8, [x0]
100222284:      cmp w8, #0x1
100222288:      b.ne    0x1002222ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4e4>
10022228c:      ldrh    w21, [x0, #0x2]
100222290:      tbnz    w21, #0xa, 0x1002222ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4e4>
100222294:      ldp w8, w9, [x22]
100222298:      cmp w8, w9
10022229c:      b.hi    0x1002222ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4e4>
1002222a0:      mov x0, x22
1002222a4:      bl  0x100227c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
1002222a8:      tbz w0, #0x0, 0x100222be8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe20>
1002222ac:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
1002222b0:      add x0, x0, #0x2d8
1002222b4:      add x1, sp, #0x18
1002222b8:      bl  0x100138058 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths_0bEB2j_>
1002222bc:      tbnz    w0, #0x0, 0x100223e50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2088>
1002222c0:      adrp    x0, 0x101125000 <__MergedGlobals+0xc0>
1002222c4:      add x0, x0, #0xc00
1002222c8:      ldr x8, [x0]
1002222cc:      blr x8
1002222d0:      mov x24, x0
1002222d4:      ldrb    w8, [x0, #0x20]
1002222d8:      cbnz    w8, 0x100223a8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cc4>
1002222dc:      ldr x8, [x24]
1002222e0:      cbnz    x8, 0x100223ab0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ce8>
1002222e4:      mov x8, #-0x1               ; =-1
1002222e8:      str x8, [x24]
1002222ec:      mov x0, x24
1002222f0:      ldr x8, [x0, #0x8]!
1002222f4:      ldr x21, [x24, #0x18]
1002222f8:      cmp x21, x8
1002222fc:      b.ne    0x100222304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x53c>
100222300:      bl  0x100ccfe40 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecjE8grow_oneCs3HfcutmYuk_10swc_common>
100222304:      ldr x8, [x24, #0x10]
100222308:      str x22, [x8, x21, lsl #3]
10022230c:      add x8, x21, #0x1
100222310:      str x8, [x24, #0x18]
100222314:      ldr x8, [x24]
100222318:      add x8, x8, #0x1
10022231c:      str x8, [x24]
100222320:      ldr w8, [x22]
100222324:      str x8, [sp, #0x10]
100222328:      mov x0, x22
10022232c:      bl  0x10022ae58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100222330:      cbz x0, 0x10022233c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x574>
100222334:      ldrh    w8, [x0, #0x2]
100222338:      tbnz    w8, #0xa, 0x100222370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5a8>
10022233c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
100222340:      ldrb    w8, [x8, #0xad1]
100222344:      cbnz    w8, 0x100222370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5a8>
100222348:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
10022234c:      ldrb    w8, [x8, #0xad3]
100222350:      cbnz    w8, 0x100222370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5a8>
100222354:      ldp w8, w9, [x22]
100222358:      cmp w8, w9
10022235c:      b.hi    0x100222370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5a8>
100222360:      mov x0, x22
100222364:      bl  0x1009aa1a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
100222368:      cmp x0, #0x1
10022236c:      b.ne    0x100222d30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf68>
100222370:      adrp    x27, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100222374:      add x27, x27, #0xfe0
100222378:      ldr x8, [x27]
10022237c:      cmn x8, #0x1
100222380:      b.eq    0x1002223b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5ec>
100222384:      mrs x9, TPIDRRO_EL0
100222388:      and x9, x9, #0xfffffffffffffff8
10022238c:      ldr x8, [x9, x8, lsl #3]
100222390:      cbz x8, 0x1002223b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5ec>
100222394:      ldr x8, [x8, #0x19e8]
100222398:      cbz x8, 0x1002223b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5ec>
10022239c:      ldr x9, [x8]
1002223a0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1002223a4:      cmp x9, x10
1002223a8:      b.hs    0x100223a10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c48>
1002223ac:      ldr x21, [x8, #0x18]
1002223b0:      b   0x1002223c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x5fc>
1002223b4:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
1002223b8:      add x0, x0, #0xc30
1002223bc:      bl  0x10013582c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1002223c0:      mov x21, x0
1002223c4:      stur    x21, [x29, #-0x68]
1002223c8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1002223cc:      stp x22, x8, [sp, #0x38]
1002223d0:      mov w8, #0x1                ; =1
1002223d4:      str x8, [sp, #0x30]
1002223d8:      add x0, sp, #0x30
1002223dc:      bl  0x1001d74b8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1002223e0:      mov x22, x0
1002223e4:      ldr x23, [x19, #0x10]
1002223e8:      ldr x8, [x19]
1002223ec:      cmp x8, x23
1002223f0:      b.eq    0x100223a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c8c>
1002223f4:      ldr x8, [x19, #0x8]
1002223f8:      mov w9, #0x5b               ; =91
1002223fc:      strb    w9, [x8, x23]
100222400:      add x23, x23, #0x1
100222404:      str x23, [x19, #0x10]
100222408:      ldr x8, [sp, #0x10]
10022240c:      cbz w8, 0x100222ae8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd20>
100222410:      stp x21, x24, [sp]
100222414:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100222418:      add x0, x0, #0xe0
10022241c:      ldr x8, [x0]
100222420:      blr x8
100222424:      mov x21, x0
100222428:      adrp    x0, 0x101125000 <__MergedGlobals+0xc0>
10022242c:      add x0, x0, #0xbb8
100222430:      ldr x8, [x0]
100222434:      blr x8
100222438:      mov x25, x0
10022243c:      mov x26, #0x0               ; =0
100222440:      b   0x100222458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x690>
100222444:      str xzr, [x8]
100222448:      add x26, x26, #0x1
10022244c:      ldr x8, [sp, #0x10]
100222450:      cmp x8, x26
100222454:      b.eq    0x100222ae0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd18>
100222458:      cbz x26, 0x100222480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6b8>
10022245c:      ldr x23, [x19, #0x10]
100222460:      ldr x8, [x19]
100222464:      cmp x8, x23
100222468:      b.eq    0x1002229b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbe8>
10022246c:      ldr x8, [x19, #0x8]
100222470:      mov w9, #0x2c               ; =44
100222474:      strb    w9, [x8, x23]
100222478:      add x8, x23, #0x1
10022247c:      str x8, [x19, #0x10]
100222480:      ldr x8, [x27]
100222484:      cmn x8, #0x1
100222488:      b.eq    0x1002224ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x724>
10022248c:      mrs x9, TPIDRRO_EL0
100222490:      and x9, x9, #0xfffffffffffffff8
100222494:      ldr x8, [x9, x8, lsl #3]
100222498:      cbz x8, 0x1002224ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x724>
10022249c:      ldr x8, [x8, #0x19e8]
1002224a0:      cbz x8, 0x1002224ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x724>
1002224a4:      ldr x9, [x8]
1002224a8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1002224ac:      cmp x9, x10
1002224b0:      b.hs    0x100223c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e7c>
1002224b4:      add x10, x9, #0x1
1002224b8:      str x10, [x8]
1002224bc:      ldr x10, [x8, #0x18]
1002224c0:      cmp x22, x10
1002224c4:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
1002224c8:      ldr x10, [x8, #0x10]
1002224cc:      mov w11, #0x18              ; =24
1002224d0:      madd    x10, x22, x11, x10
1002224d4:      ldr x11, [x10]
1002224d8:      cmp x11, #0x1
1002224dc:      b.ne    0x100223c50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e88>
1002224e0:      ldr x0, [x10, #0x8]
1002224e4:      str x9, [x8]
1002224e8:      b   0x100222538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x770>
1002224ec:      ldrb    w8, [x21, #0x20]
1002224f0:      cbnz    w8, 0x1002229cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc04>
1002224f4:      ldr x8, [x21]
1002224f8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002224fc:      cmp x8, x9
100222500:      b.hs    0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
100222504:      add x9, x8, #0x1
100222508:      str x9, [x21]
10022250c:      ldr x9, [x21, #0x18]
100222510:      cmp x22, x9
100222514:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
100222518:      ldr x9, [x21, #0x10]
10022251c:      mov w10, #0x18              ; =24
100222520:      madd    x9, x22, x10, x9
100222524:      ldr x10, [x9]
100222528:      cmp x10, #0x1
10022252c:      b.ne    0x1002236e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x191c>
100222530:      ldr x0, [x9, #0x8]
100222534:      str x8, [x21]
100222538:      mov x1, x26
10022253c:      bl  0x1005c6d7c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime5array8indexing11proto_chain14array_spec_get>
100222540:      mov.16b v8, v0
100222544:      fmov    x23, d8
100222548:      mov x8, #0x1                ; =1
10022254c:      movk    x8, #0x7ffc, lsl #48
100222550:      cmp x23, x8
100222554:      mov x8, #0x10               ; =16
100222558:      movk    x8, #0x7ffc, lsl #48
10022255c:      ccmp    x23, x8, #0x4, ne
100222560:      b.ne    0x100222598 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7d0>
100222564:      ldr x1, [x19, #0x10]
100222568:      ldr x8, [x19]
10022256c:      sub x8, x8, x1
100222570:      cmp x8, #0x3
100222574:      b.ls    0x100222994 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbcc>
100222578:      ldr x8, [x19, #0x8]
10022257c:      mov w9, #0x756e             ; =30062
100222580:      movk    w9, #0x6c6c, lsl #16
100222584:      str w9, [x8, x1]
100222588:      ldr x8, [x19, #0x10]
10022258c:      add x8, x8, #0x4
100222590:      str x8, [x19, #0x10]
100222594:      b   0x100222448 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x680>
100222598:      and x24, x23, #0xffff000000000000
10022259c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1002225a0:      cmp x24, x8
1002225a4:      b.ne    0x1002225dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x814>
1002225a8:      and x8, x23, #0xffffffffffff
1002225ac:      cmp x8, #0x100, lsl #12     ; =0x100000
1002225b0:      b.lo    0x1002225c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x800>
1002225b4:      ldr w8, [x8, #0xc]
1002225b8:      mov w9, #0x4f53             ; =20307
1002225bc:      movk    w9, #0x434c, lsl #16
1002225c0:      cmp w8, w9
1002225c4:      b.eq    0x100222564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x79c>
1002225c8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1002225cc:      cmp x24, x8
1002225d0:      b.ne    0x100222608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x840>
1002225d4:      and x0, x23, #0xffffffffffff
1002225d8:      b   0x100222630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x868>
1002225dc:      lsr x8, x23, #52
1002225e0:      cbnz    x8, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
1002225e4:      and x8, x23, #0x7
1002225e8:      cbz x23, 0x100222614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x84c>
1002225ec:      cbnz    x8, 0x100222614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x84c>
1002225f0:      mov x0, x23
1002225f4:      bl  0x100226c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
1002225f8:      mov x8, x23
1002225fc:      tbnz    w0, #0x0, 0x1002225ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7e4>
100222600:      mov x8, #0x0                ; =0
100222604:      b   0x100222614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x84c>
100222608:      lsr x8, x23, #52
10022260c:      cbnz    x8, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222610:      and x8, x23, #0x7
100222614:      cbz x23, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222618:      cbnz    x8, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
10022261c:      mov x0, x23
100222620:      bl  0x100226c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100222624:      mov x8, x0
100222628:      mov x0, x23
10022262c:      tbz w8, #0x0, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222630:      cmp x0, #0x100, lsl #12     ; =0x100000
100222634:      b.lo    0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222638:      adrp    x8, 0x10116c000 <_out_buf+0x3f08>
10022263c:      add x8, x8, #0x68d
100222640:      ldaprb  w8, [x8]
100222644:      cbz w8, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222648:      lsr x8, x0, #3
10022264c:      mov x9, #0x7c15             ; =31765
100222650:      movk    x9, #0x7f4a, lsl #16
100222654:      movk    x9, #0x79b9, lsl #32
100222658:      movk    x9, #0x9e37, lsl #48
10022265c:      mul x8, x8, x9
100222660:      lsr x9, x8, #54
100222664:      lsr x10, x8, #60
100222668:      adrp    x11, 0x10116c000 <_out_buf+0x3f08>
10022266c:      add x11, x11, #0x690
100222670:      add x10, x11, x10, lsl #3
100222674:      ldapr   x10, [x10]
100222678:      lsr x9, x10, x9
10022267c:      tbz w9, #0x0, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222680:      lsr x9, x8, #44
100222684:      ubfx    x10, x8, #50, #4
100222688:      add x10, x11, x10, lsl #3
10022268c:      ldapr   x10, [x10]
100222690:      lsr x9, x10, x9
100222694:      tbz w9, #0x0, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
100222698:      lsr x9, x8, #34
10022269c:      ubfx    x8, x8, #40, #4
1002226a0:      adrp    x10, 0x10116c000 <_out_buf+0x3f08>
1002226a4:      add x10, x10, #0x690
1002226a8:      add x8, x10, x8, lsl #3
1002226ac:      ldapr   x8, [x8]
1002226b0:      lsr x8, x8, x9
1002226b4:      tbz w8, #0x0, 0x1002226c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x8f8>
1002226b8:      bl  0x10018c218 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6symbol25is_registered_symbol_slow>
1002226bc:      tbnz    w0, #0x0, 0x100222564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x79c>
1002226c0:      ldr x8, [x27]
1002226c4:      cmn x8, #0x1
1002226c8:      b.eq    0x1002226f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x930>
1002226cc:      mrs x9, TPIDRRO_EL0
1002226d0:      and x9, x9, #0xfffffffffffffff8
1002226d4:      ldr x8, [x9, x8, lsl #3]
1002226d8:      cbz x8, 0x1002226f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x930>
1002226dc:      ldr x8, [x8, #0x19e8]
1002226e0:      cbz x8, 0x1002226f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x930>
1002226e4:      ldr x9, [x8], #0x18
1002226e8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1002226ec:      cmp x9, x10
1002226f0:      b.lo    0x100222714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x94c>
1002226f4:      b   0x100223a10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c48>
1002226f8:      ldrb    w8, [x21, #0x20]
1002226fc:      cbnz    w8, 0x100222a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc5c>
100222700:      ldr x9, [x21]
100222704:      add x8, x21, #0x18
100222708:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10022270c:      cmp x9, x10
100222710:      b.hs    0x10022378c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19c4>
100222714:      ldr x28, [x8]
100222718:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x110>
10022271c:      add x8, x8, #0xaa8
100222720:      ldr w8, [x8]
100222724:      cbz w8, 0x100222730 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x968>
100222728:      mov x0, x23
10022272c:      bl  0x1009904f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
100222730:      ldr x8, [x27]
100222734:      cmn x8, #0x1
100222738:      b.eq    0x10022279c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9d4>
10022273c:      mrs x9, TPIDRRO_EL0
100222740:      and x9, x9, #0xfffffffffffffff8
100222744:      ldr x8, [x9, x8, lsl #3]
100222748:      cbz x8, 0x10022279c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9d4>
10022274c:      ldr x24, [x8, #0x19e8]
100222750:      cbz x24, 0x10022279c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9d4>
100222754:      ldr x8, [x24]
100222758:      cbnz    x8, 0x100223a38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c70>
10022275c:      mov x8, #-0x1               ; =-1
100222760:      str x8, [x24]
100222764:      mov x0, x24
100222768:      ldr x8, [x0, #0x8]!
10022276c:      ldr x23, [x24, #0x18]
100222770:      cmp x23, x8
100222774:      b.ne    0x10022277c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9b4>
100222778:      bl  0x100cb70f0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
10022277c:      ldr x8, [x24, #0x10]
100222780:      mov w9, #0x18               ; =24
100222784:      madd    x8, x23, x9, x8
100222788:      str xzr, [x8]
10022278c:      str d8, [x8, #0x8]
100222790:      add x8, x23, #0x1
100222794:      str x8, [x24, #0x18]
100222798:      b   0x1002227ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa24>
10022279c:      ldrb    w8, [x21, #0x20]
1002227a0:      cbnz    w8, 0x100222a58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc90>
1002227a4:      ldr x8, [x21]
1002227a8:      cbnz    x8, 0x100223798 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d0>
1002227ac:      mov x8, #-0x1               ; =-1
1002227b0:      str x8, [x21]
1002227b4:      ldr x23, [x21, #0x18]
1002227b8:      ldr x8, [x21, #0x8]
1002227bc:      cmp x23, x8
1002227c0:      b.ne    0x1002227cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa04>
1002227c4:      add x0, x21, #0x8
1002227c8:      bl  0x100cb70f0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecTyyNtNtCseUPtmYZaE8V_5gimli6common13EhFrameOffsetEE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1002227cc:      ldr x8, [x21, #0x10]
1002227d0:      mov w9, #0x18               ; =24
1002227d4:      madd    x8, x23, x9, x8
1002227d8:      str xzr, [x8]
1002227dc:      str d8, [x8, #0x8]
1002227e0:      add x8, x23, #0x1
1002227e4:      str x8, [x21, #0x18]
1002227e8:      mov x24, x21
1002227ec:      ldr x8, [x24]
1002227f0:      add x8, x8, #0x1
1002227f4:      str x8, [x24]
1002227f8:      str x26, [sp, #0x60]
1002227fc:      ldrb    w8, [x25, #0x20]
100222800:      cbnz    w8, 0x1002229fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc34>
100222804:      ldr x8, [x25]
100222808:      cbnz    x8, 0x100223780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19b8>
10022280c:      mov x8, #-0x1               ; =-1
100222810:      str x8, [x25]
100222814:      str xzr, [x25, #0x18]
100222818:      add x8, sp, #0x60
10022281c:      str x8, [sp, #0x30]
100222820:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
100222824:      add x8, x8, #0xf80
100222828:      str x8, [sp, #0x38]
10022282c:      add x0, x25, #0x8
100222830:      add x3, sp, #0x30
100222834:      adrp    x1, 0x101093000 <_anon.438b28c8644b10f28676d307896bf03a.683>
100222838:      add x1, x1, #0x590
10022283c:      adrp    x2, 0x100ee2000 <_anon.438b28c8644b10f28676d307896bf03a.98+0x28>
100222840:      add x2, x2, #0x794
100222844:      bl  0x10002cf10 <__RNvNtCsjgY6bXVaRmE_4core3fmt5write>
100222848:      ldr x8, [x25]
10022284c:      add x8, x8, #0x1
100222850:      str x8, [x25]
100222854:      ldr x8, [x27]
100222858:      cmn x8, #0x1
10022285c:      b.eq    0x1002228d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb0c>
100222860:      mrs x9, TPIDRRO_EL0
100222864:      and x9, x9, #0xfffffffffffffff8
100222868:      ldr x8, [x9, x8, lsl #3]
10022286c:      cbz x8, 0x1002228d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb0c>
100222870:      ldr x8, [x8, #0x19e8]
100222874:      cbz x8, 0x1002228d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb0c>
100222878:      ldr x9, [x8]
10022287c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100222880:      cmp x9, x10
100222884:      b.hs    0x100223c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e7c>
100222888:      add x10, x9, #0x1
10022288c:      str x10, [x8]
100222890:      ldr x10, [x8, #0x18]
100222894:      cmp x23, x10
100222898:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
10022289c:      ldr x10, [x8, #0x10]
1002228a0:      mov w11, #0x18              ; =24
1002228a4:      madd    x10, x23, x11, x10
1002228a8:      ldr x11, [x10]
1002228ac:      cbnz    x11, 0x100223a44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c7c>
1002228b0:      ldr d0, [x10, #0x8]
1002228b4:      str x9, [x8]
1002228b8:      add w1, w20, #0x1
1002228bc:      mov x0, x19
1002228c0:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1002228c4:      ldr x8, [x27]
1002228c8:      cmn x8, #0x1
1002228cc:      b.ne    0x100222934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb6c>
1002228d0:      b   0x100222968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xba0>
1002228d4:      ldrb    w8, [x21, #0x20]
1002228d8:      cbnz    w8, 0x100222a80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xcb8>
1002228dc:      ldr x8, [x21]
1002228e0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002228e4:      cmp x8, x9
1002228e8:      b.hs    0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
1002228ec:      add x9, x8, #0x1
1002228f0:      str x9, [x21]
1002228f4:      ldr x9, [x21, #0x18]
1002228f8:      cmp x23, x9
1002228fc:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
100222900:      ldr x9, [x21, #0x10]
100222904:      mov w10, #0x18              ; =24
100222908:      madd    x9, x23, x10, x9
10022290c:      ldr x10, [x9]
100222910:      cbnz    x10, 0x1002237a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19dc>
100222914:      ldr d0, [x9, #0x8]
100222918:      str x8, [x21]
10022291c:      add w1, w20, #0x1
100222920:      mov x0, x19
100222924:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100222928:      ldr x8, [x27]
10022292c:      cmn x8, #0x1
100222930:      b.eq    0x100222968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xba0>
100222934:      mrs x9, TPIDRRO_EL0
100222938:      and x9, x9, #0xfffffffffffffff8
10022293c:      ldr x8, [x9, x8, lsl #3]
100222940:      cbz x8, 0x100222968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xba0>
100222944:      ldr x8, [x8, #0x19e8]
100222948:      cbz x8, 0x100222968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xba0>
10022294c:      ldr x9, [x8]
100222950:      cbnz    x9, 0x100223cf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
100222954:      ldr x9, [x8, #0x18]
100222958:      cmp x28, x9
10022295c:      b.hi    0x100222444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x67c>
100222960:      str x28, [x8, #0x18]
100222964:      b   0x100222444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x67c>
100222968:      ldrb    w8, [x21, #0x20]
10022296c:      cbnz    w8, 0x100222ab0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xce8>
100222970:      ldr x8, [x21]
100222974:      cbnz    x8, 0x100222ad4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd0c>
100222978:      add x8, x21, #0x18
10022297c:      ldr x8, [x8]
100222980:      cmp x28, x8
100222984:      b.hi    0x100222448 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x680>
100222988:      add x8, x21, #0x18
10022298c:      str x28, [x8]
100222990:      b   0x100222448 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x680>
100222994:      mov x0, x19
100222998:      mov w2, #0x4                ; =4
10022299c:      mov w3, #0x1                ; =1
1002229a0:      mov w4, #0x1                ; =1
1002229a4:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002229a8:      ldr x1, [x19, #0x10]
1002229ac:      b   0x100222578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x7b0>
1002229b0:      mov x0, x19
1002229b4:      mov x1, x23
1002229b8:      mov w2, #0x1                ; =1
1002229bc:      mov w3, #0x1                ; =1
1002229c0:      mov w4, #0x1                ; =1
1002229c4:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002229c8:      b   0x10022246c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x6a4>
1002229cc:      cmp w8, #0x2
1002229d0:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
1002229d4:      mov x0, x21
1002229d8:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1002229dc:      add x1, x1, #0x824
1002229e0:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002229e4:      strb    wzr, [x21, #0x20]
1002229e8:      ldr x8, [x21]
1002229ec:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1002229f0:      cmp x8, x9
1002229f4:      b.lo    0x100222504 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x73c>
1002229f8:      b   0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
1002229fc:      cmp w8, #0x2
100222a00:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100222a04:      mov x0, x25
100222a08:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100222a0c:      add x1, x1, #0xbb4
100222a10:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100222a14:      strb    wzr, [x25, #0x20]
100222a18:      ldr x8, [x25]
100222a1c:      cbz x8, 0x10022280c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xa44>
100222a20:      b   0x100223780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19b8>
100222a24:      cmp w8, #0x2
100222a28:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100222a2c:      mov x0, x21
100222a30:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100222a34:      add x1, x1, #0x824
100222a38:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100222a3c:      strb    wzr, [x21, #0x20]
100222a40:      ldr x9, [x21]
100222a44:      add x8, x21, #0x18
100222a48:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100222a4c:      cmp x9, x10
100222a50:      b.lo    0x100222714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x94c>
100222a54:      b   0x10022378c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19c4>
100222a58:      cmp w8, #0x2
100222a5c:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100222a60:      mov x0, x21
100222a64:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100222a68:      add x1, x1, #0x824
100222a6c:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100222a70:      strb    wzr, [x21, #0x20]
100222a74:      ldr x8, [x21]
100222a78:      cbz x8, 0x1002227ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x9e4>
100222a7c:      b   0x100223798 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19d0>
100222a80:      cmp w8, #0x2
100222a84:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100222a88:      mov x0, x21
100222a8c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100222a90:      add x1, x1, #0x824
100222a94:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100222a98:      strb    wzr, [x21, #0x20]
100222a9c:      ldr x8, [x21]
100222aa0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100222aa4:      cmp x8, x9
100222aa8:      b.lo    0x1002228ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xb24>
100222aac:      b   0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
100222ab0:      cmp w8, #0x2
100222ab4:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100222ab8:      mov x0, x21
100222abc:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100222ac0:      add x1, x1, #0x824
100222ac4:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100222ac8:      strb    wzr, [x21, #0x20]
100222acc:      ldr x8, [x21]
100222ad0:      cbz x8, 0x100222978 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xbb0>
100222ad4:      adrp    x0, 0x101095000 <_anon.438b28c8644b10f28676d307896bf03a.1140>
100222ad8:      add x0, x0, #0x2b8
100222adc:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100222ae0:      ldr x23, [x19, #0x10]
100222ae4:      ldp x21, x24, [sp]
100222ae8:      ldr x8, [x19]
100222aec:      cmp x8, x23
100222af0:      b.eq    0x100223a70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ca8>
100222af4:      ldr x8, [x19, #0x8]
100222af8:      mov w9, #0x5d               ; =93
100222afc:      strb    w9, [x8, x23]
100222b00:      add x8, x23, #0x1
100222b04:      str x8, [x19, #0x10]
100222b08:      ldr x8, [x27]
100222b0c:      cmn x8, #0x1
100222b10:      b.eq    0x100222b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd84>
100222b14:      mrs x9, TPIDRRO_EL0
100222b18:      and x9, x9, #0xfffffffffffffff8
100222b1c:      ldr x8, [x9, x8, lsl #3]
100222b20:      cbz x8, 0x100222b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd84>
100222b24:      ldr x8, [x8, #0x19e8]
100222b28:      cbz x8, 0x100222b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd84>
100222b2c:      ldr x9, [x8]
100222b30:      cbnz    x9, 0x100223cf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
100222b34:      ldr x9, [x8, #0x18]
100222b38:      cmp x21, x9
100222b3c:      b.hi    0x100222b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd7c>
100222b40:      str x21, [x8, #0x18]
100222b44:      str xzr, [x8]
100222b48:      b   0x100222b5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd94>
100222b4c:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100222b50:      add x0, x0, #0xc30
100222b54:      sub x1, x29, #0x68
100222b58:      bl  0x100135c08 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100222b5c:      ldrb    w8, [x24, #0x20]
100222b60:      cbnz    w8, 0x100223abc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cf4>
100222b64:      ldr x8, [x24]
100222b68:      cbnz    x8, 0x100223d34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f6c>
100222b6c:      ldr x8, [x24, #0x18]
100222b70:      cbz x8, 0x100222b7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xdb4>
100222b74:      sub x8, x8, #0x1
100222b78:      str x8, [x24, #0x18]
100222b7c:      str xzr, [x24]
100222b80:      b   0x100222188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3c0>
100222b84:      lsr x8, x21, #52
100222b88:      cbnz    x8, 0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222b8c:      cbz x21, 0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222b90:      and x8, x21, #0x7
100222b94:      cbnz    x8, 0x100222bac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xde4>
100222b98:      mov x0, x21
100222b9c:      mov.16b v8, v0
100222ba0:      bl  0x100226c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100222ba4:      mov.16b v0, v8
100222ba8:      tbnz    w0, #0x0, 0x1002221f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x42c>
100222bac:      add w1, w20, #0x1
100222bb0:      mov x0, x19
100222bb4:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100222bb8:      adrp    x0, 0x101125000 <__MergedGlobals+0xc0>
100222bbc:      add x0, x0, #0xc18
100222bc0:      ldr x8, [x0]
100222bc4:      blr x8
100222bc8:      strb    wzr, [x0]
100222bcc:      b   0x100222188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3c0>
100222bd0:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100222bd4:      lsr x1, x22, #20
100222bd8:      ldr x8, [x0, #0x10]
100222bdc:      ldrb    w9, [x8, #0x28]
100222be0:      tbnz    w9, #0x0, 0x100221e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xac>
100222be4:      b   0x100221e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xc8>
100222be8:      tbnz    w21, #0x7, 0x100222c08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe40>
100222bec:      mov x8, x22
100222bf0:      ldr w9, [x8], #0x8
100222bf4:      add x9, x8, x9, lsl #3
100222bf8:      stp x8, x9, [sp, #0x30]
100222bfc:      add x0, sp, #0x30
100222c00:      bl  0x1001ce7f4 <__RINvXs2J_NtNtCsjgY6bXVaRmE_4core5slice4iterINtB7_4IterdENtNtNtNtBb_4iter6traits8iterator8Iterator3allNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json25stringify_primitive_array8try_emit0EB1J_>
100222c04:      cbz w0, 0x1002222ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x4e4>
100222c08:      ldurh   w24, [x22, #-0x6]
100222c0c:      ldr w21, [x22]
100222c10:      ldr x20, [x19, #0x10]
100222c14:      ldr x8, [x19]
100222c18:      cmp x8, x20
100222c1c:      b.eq    0x100223d40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f78>
100222c20:      ldr x8, [x19, #0x8]
100222c24:      mov w9, #0x5b               ; =91
100222c28:      strb    w9, [x8, x20]
100222c2c:      add x20, x20, #0x1
100222c30:      str x20, [x19, #0x10]
100222c34:      lsl x23, x21, #3
100222c38:      tbnz    w24, #0x7, 0x100222cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xee8>
100222c3c:      cbz w21, 0x1002237f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a30>
100222c40:      ldr d0, [x22, #0x8]
100222c44:      fmov    x0, d0
100222c48:      mov x8, #-0x7ffc000000000001 ; =-9222246136947933185
100222c4c:      add x8, x0, x8
100222c50:      cmp x8, #0x2
100222c54:      b.lo    0x1002237c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19f8>
100222c58:      mov x8, #0x4                ; =4
100222c5c:      movk    x8, #0x7ffc, lsl #48
100222c60:      cmp x0, x8
100222c64:      b.eq    0x1002236f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x192c>
100222c68:      mov x8, #0x3                ; =3
100222c6c:      movk    x8, #0x7ffc, lsl #48
100222c70:      cmp x0, x8
100222c74:      b.ne    0x100223714 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x194c>
100222c78:      ldr x8, [x19]
100222c7c:      sub x8, x8, x20
100222c80:      cmp x8, #0x4
100222c84:      b.ls    0x100223e10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2048>
100222c88:      ldr x8, [x19, #0x8]
100222c8c:      add x8, x8, x20
100222c90:      mov w9, #0x65               ; =101
100222c94:      strb    w9, [x8, #0x4]
100222c98:      mov w9, #0x6166             ; =24934
100222c9c:      movk    w9, #0x736c, lsl #16
100222ca0:      str w9, [x8]
100222ca4:      ldr x8, [x19, #0x10]
100222ca8:      add x8, x8, #0x5
100222cac:      b   0x1002237e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a20>
100222cb0:      cbz w21, 0x1002237f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a30>
100222cb4:      ldr d0, [x22, #0x8]
100222cb8:      mov x0, x19
100222cbc:      bl  0x10021294c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100222cc0:      cmp w21, #0x1
100222cc4:      b.eq    0x1002237f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
100222cc8:      add x21, x22, #0x10
100222ccc:      sub x22, x23, #0x8
100222cd0:      mov w23, #0x2c              ; =44
100222cd4:      ldr d0, [x21], #0x8
100222cd8:      ldr x20, [x19, #0x10]
100222cdc:      ldr x8, [x19]
100222ce0:      cmp x8, x20
100222ce4:      b.eq    0x100222d0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf44>
100222ce8:      ldr x8, [x19, #0x8]
100222cec:      strb    w23, [x8, x20]
100222cf0:      add x8, x20, #0x1
100222cf4:      str x8, [x19, #0x10]
100222cf8:      mov x0, x19
100222cfc:      bl  0x10021294c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100222d00:      subs    x22, x22, #0x8
100222d04:      b.ne    0x100222cd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf0c>
100222d08:      b   0x1002237f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
100222d0c:      mov x0, x19
100222d10:      mov x1, x20
100222d14:      mov w2, #0x1                ; =1
100222d18:      mov w3, #0x1                ; =1
100222d1c:      mov w4, #0x1                ; =1
100222d20:      mov.16b v8, v0
100222d24:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100222d28:      mov.16b v0, v8
100222d2c:      b   0x100222ce8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xf20>
100222d30:      adrp    x28, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100222d34:      add x28, x28, #0xfe0
100222d38:      ldr x8, [x28]
100222d3c:      cmn x8, #0x1
100222d40:      b.eq    0x100222d74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfac>
100222d44:      mrs x9, TPIDRRO_EL0
100222d48:      and x9, x9, #0xfffffffffffffff8
100222d4c:      ldr x8, [x9, x8, lsl #3]
100222d50:      cbz x8, 0x100222d74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfac>
100222d54:      ldr x8, [x8, #0x19e8]
100222d58:      cbz x8, 0x100222d74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfac>
100222d5c:      ldr x9, [x8]
100222d60:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100222d64:      cmp x9, x10
100222d68:      b.hs    0x100223a10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c48>
100222d6c:      ldr x21, [x8, #0x18]
100222d70:      b   0x100222d84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xfbc>
100222d74:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100222d78:      add x0, x0, #0xc30
100222d7c:      bl  0x10013582c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100222d80:      mov x21, x0
100222d84:      str x21, [sp, #0x20]
100222d88:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100222d8c:      stp x22, x8, [sp, #0x38]
100222d90:      mov w8, #0x1                ; =1
100222d94:      str x8, [sp, #0x30]
100222d98:      add x0, sp, #0x30
100222d9c:      bl  0x1001d74b8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100222da0:      str x0, [sp, #0x28]
100222da4:      add x8, sp, #0x28
100222da8:      stur    x8, [x29, #-0x68]
100222dac:      ldr x8, [sp, #0x10]
100222db0:      cmp w8, #0x2
100222db4:      b.lo    0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222db8:      sub x0, x29, #0x68
100222dbc:      bl  0x1001cf49c <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths3_0B7_>
100222dc0:      fmov    x22, d0
100222dc4:      mov w8, #0x7ffd             ; =32765
100222dc8:      cmp x8, x22, lsr #48
100222dcc:      b.ne    0x100222ee8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1120>
100222dd0:      and x23, x22, #0xffffffffffff
100222dd4:      cmp x23, #0x100, lsl #12    ; =0x100000
100222dd8:      b.lo    0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222ddc:      and x0, x22, #0xffffffffffff
100222de0:      bl  0x1001eb7f4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
100222de4:      tbnz    w0, #0x0, 0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222de8:      ldr w8, [x23]
100222dec:      mov w9, #-0xff5f            ; =-65375
100222df0:      cmp w8, w9
100222df4:      b.eq    0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222df8:      sub x0, x29, #0x68
100222dfc:      bl  0x1001cf49c <__RNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths3_0B7_>
100222e00:      bl  0x1002492f4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100222e04:      cmp x0, #0x1
100222e08:      b.eq    0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222e0c:      add x0, sp, #0x30
100222e10:      mov x1, x22
100222e14:      bl  0x100217480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template27build_shape_prefix_template>
100222e18:      ldr x8, [sp, #0x30]
100222e1c:      cmn x8, #0x1
100222e20:      b.eq    0x100222f14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x114c>
100222e24:      ldr x22, [x19, #0x10]
100222e28:      ldr x8, [x19]
100222e2c:      cmp x8, x22
100222e30:      b.eq    0x100223e90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x20c8>
100222e34:      ldr x8, [x19, #0x8]
100222e38:      mov w9, #0x5b               ; =91
100222e3c:      strb    w9, [x8, x22]
100222e40:      add x8, x22, #0x1
100222e44:      str x8, [x19, #0x10]
100222e48:      str xzr, [sp, #0x60]
100222e4c:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
100222e50:      add x0, x0, #0x228
100222e54:      add x1, sp, #0x60
100222e58:      bl  0x1001641ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100222e5c:      ldr x8, [x28]
100222e60:      cmn x8, #0x1
100222e64:      b.eq    0x100223ad0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
100222e68:      mrs x9, TPIDRRO_EL0
100222e6c:      and x9, x9, #0xfffffffffffffff8
100222e70:      ldr x8, [x9, x8, lsl #3]
100222e74:      cbz x8, 0x100223ad0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
100222e78:      ldr x8, [x8, #0x19e8]
100222e7c:      cbz x8, 0x100223ad0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d08>
100222e80:      ldr x9, [x8]
100222e84:      mov x10, #0x7ffffffffffffffe ; =9223372036854775806
100222e88:      cmp x9, x10
100222e8c:      b.hi    0x100223c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e7c>
100222e90:      ldr x10, [sp, #0x28]
100222e94:      add x11, x9, #0x1
100222e98:      str x11, [x8]
100222e9c:      ldr x11, [x8, #0x18]
100222ea0:      cmp x10, x11
100222ea4:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
100222ea8:      ldr x11, [x8, #0x10]
100222eac:      mov w12, #0x18              ; =24
100222eb0:      madd    x10, x10, x12, x11
100222eb4:      ldr x11, [x10]
100222eb8:      cmp x11, #0x1
100222ebc:      b.ne    0x100223c50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e88>
100222ec0:      ldr x0, [x10, #0x8]
100222ec4:      str x9, [x8]
100222ec8:      b   0x100223ae0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d18>
100222ecc:      mov x0, x19
100222ed0:      mov w2, #0x2                ; =2
100222ed4:      mov w3, #0x1                ; =1
100222ed8:      mov w4, #0x1                ; =1
100222edc:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100222ee0:      ldr x1, [x19, #0x10]
100222ee4:      b   0x100222170 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3a8>
100222ee8:      lsr x8, x22, #52
100222eec:      cbnz    x8, 0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222ef0:      cbz x22, 0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222ef4:      and x8, x22, #0x7
100222ef8:      cbnz    x8, 0x100222f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1144>
100222efc:      mov x0, x22
100222f00:      bl  0x100226c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
100222f04:      mov x23, x22
100222f08:      cbnz    w0, 0x100222dd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x100c>
100222f0c:      mov x8, #-0x1               ; =-1
100222f10:      str x8, [sp, #0x30]
100222f14:      ldr x21, [x19, #0x10]
100222f18:      ldr x8, [x19]
100222f1c:      cmp x8, x21
100222f20:      b.eq    0x100223db8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ff0>
100222f24:      ldr x8, [x19, #0x8]
100222f28:      mov w9, #0x5b               ; =91
100222f2c:      strb    w9, [x8, x21]
100222f30:      add x21, x21, #0x1
100222f34:      str x21, [x19, #0x10]
100222f38:      ldr x8, [sp, #0x10]
100222f3c:      cbz w8, 0x100223698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18d0>
100222f40:      mov x23, #0x1               ; =1
100222f44:      movk    x23, #0x7ffc, lsl #48
100222f48:      mov w12, #0x756e            ; =30062
100222f4c:      movk    w12, #0x6c6c, lsl #16
100222f50:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
100222f54:      add x0, x0, #0xe0
100222f58:      ldr x8, [x0]
100222f5c:      blr x8
100222f60:      mov x21, x0
100222f64:      adrp    x0, 0x101125000 <__MergedGlobals+0xc0>
100222f68:      add x0, x0, #0xc18
100222f6c:      ldr x8, [x0]
100222f70:      blr x8
100222f74:      str x0, [sp, #0x8]
100222f78:      mov x22, #0x0               ; =0
100222f7c:      mov x27, #0x7fffffffffffffff ; =9223372036854775807
100222f80:      mov w24, #0x18              ; =24
100222f84:      mov w13, #0x2c              ; =44
100222f88:      b   0x100222fc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11fc>
100222f8c:      ldr x1, [x19, #0x10]
100222f90:      ldr x8, [x19]
100222f94:      sub x8, x8, x1
100222f98:      cmp x8, #0x3
100222f9c:      b.ls    0x1002232b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14ec>
100222fa0:      ldr x8, [x19, #0x8]
100222fa4:      str w12, [x8, x1]
100222fa8:      ldr x8, [x19, #0x10]
100222fac:      add x8, x8, #0x4
100222fb0:      str x8, [x19, #0x10]
100222fb4:      add x22, x22, #0x1
100222fb8:      ldr x8, [sp, #0x10]
100222fbc:      cmp x8, x22
100222fc0:      b.eq    0x100223694 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18cc>
100222fc4:      cbz x22, 0x100222fe8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1220>
100222fc8:      ldr x25, [x19, #0x10]
100222fcc:      ldr x8, [x19]
100222fd0:      cmp x8, x25
100222fd4:      b.eq    0x100223470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16a8>
100222fd8:      ldr x8, [x19, #0x8]
100222fdc:      strb    w13, [x8, x25]
100222fe0:      add x8, x25, #0x1
100222fe4:      str x8, [x19, #0x10]
100222fe8:      ldr x8, [x28]
100222fec:      cmn x8, #0x1
100222ff0:      b.eq    0x100223050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1288>
100222ff4:      mrs x9, TPIDRRO_EL0
100222ff8:      and x9, x9, #0xfffffffffffffff8
100222ffc:      ldr x8, [x9, x8, lsl #3]
100223000:      cbz x8, 0x100223050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1288>
100223004:      ldr x8, [x8, #0x19e8]
100223008:      cbz x8, 0x100223050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1288>
10022300c:      ldr x9, [x8]
100223010:      cmp x9, x27
100223014:      b.hs    0x100223c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e7c>
100223018:      ldr x10, [sp, #0x28]
10022301c:      add x11, x9, #0x1
100223020:      str x11, [x8]
100223024:      ldr x11, [x8, #0x18]
100223028:      cmp x10, x11
10022302c:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
100223030:      ldr x11, [x8, #0x10]
100223034:      madd    x10, x10, x24, x11
100223038:      ldr x11, [x10]
10022303c:      cmp x11, #0x1
100223040:      b.ne    0x100223c50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e88>
100223044:      ldr x10, [x10, #0x8]
100223048:      str x9, [x8]
10022304c:      b   0x100223098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12d0>
100223050:      ldr x25, [sp, #0x28]
100223054:      ldrb    w8, [x21, #0x20]
100223058:      cbnz    w8, 0x100223498 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16d0>
10022305c:      ldr x8, [x21]
100223060:      cmp x8, x27
100223064:      b.hs    0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
100223068:      add x9, x8, #0x1
10022306c:      str x9, [x21]
100223070:      ldr x9, [x21, #0x18]
100223074:      cmp x25, x9
100223078:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
10022307c:      ldr x9, [x21, #0x10]
100223080:      madd    x9, x25, x24, x9
100223084:      ldr x10, [x9]
100223088:      cmp x10, #0x1
10022308c:      b.ne    0x1002236e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x191c>
100223090:      ldr x10, [x9, #0x8]
100223094:      str x8, [x21]
100223098:      add x8, x10, x22, lsl #3
10022309c:      ldr d8, [x8, #0x8]
1002230a0:      fmov    x26, d8
1002230a4:      cmp x26, x23
1002230a8:      b.eq    0x100222f8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11c4>
1002230ac:      and x8, x26, #0xffff000000000000
1002230b0:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1002230b4:      cmp x8, x9
1002230b8:      b.eq    0x1002230f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x132c>
1002230bc:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1002230c0:      cmp x8, x9
1002230c4:      b.ne    0x100223188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13c0>
1002230c8:      and x8, x26, #0xffffffffffff
1002230cc:      cmp x8, #0x1, lsl #12       ; =0x1000
1002230d0:      b.lo    0x100222f8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11c4>
1002230d4:      ldr w2, [x8, #0x4]
1002230d8:      add x1, x8, #0x14
1002230dc:      mov x0, x19
1002230e0:      bl  0x100213c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1002230e4:      mov w13, #0x2c              ; =44
1002230e8:      mov w12, #0x756e            ; =30062
1002230ec:      movk    w12, #0x6c6c, lsl #16
1002230f0:      b   0x100222fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11ec>
1002230f4:      strb    wzr, [sp, #0x5c]
1002230f8:      str wzr, [sp, #0x58]
1002230fc:      ubfx    x1, x26, #40, #8
100223100:      cbz x1, 0x100223150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1388>
100223104:      strb    w26, [sp, #0x58]
100223108:      cmp x1, #0x1
10022310c:      b.eq    0x100223150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1388>
100223110:      lsr x8, x26, #8
100223114:      strb    w8, [sp, #0x59]
100223118:      cmp x1, #0x2
10022311c:      b.eq    0x100223150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1388>
100223120:      lsr x8, x26, #16
100223124:      strb    w8, [sp, #0x5a]
100223128:      cmp x1, #0x3
10022312c:      b.eq    0x100223150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1388>
100223130:      lsr x8, x26, #24
100223134:      strb    w8, [sp, #0x5b]
100223138:      cmp x1, #0x4
10022313c:      b.eq    0x100223150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1388>
100223140:      lsr x8, x26, #32
100223144:      strb    w8, [sp, #0x5c]
100223148:      cmp x1, #0x5
10022314c:      b.ne    0x100223ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2100>
100223150:      add x8, sp, #0x60
100223154:      add x0, sp, #0x58
100223158:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
10022315c:      ldr x8, [sp, #0x60]
100223160:      cbz x8, 0x1002231e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1418>
100223164:      ldr x1, [x19, #0x10]
100223168:      ldr x8, [x19]
10022316c:      sub x8, x8, x1
100223170:      cmp x8, #0x3
100223174:      b.ls    0x10022353c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1774>
100223178:      mov w12, #0x756e            ; =30062
10022317c:      movk    w12, #0x6c6c, lsl #16
100223180:      mov w13, #0x2c              ; =44
100223184:      b   0x100222fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
100223188:      mov x9, #0x2                ; =2
10022318c:      movk    x9, #0x7ffc, lsl #48
100223190:      cmp x26, x9
100223194:      b.eq    0x100222f8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11c4>
100223198:      mov x9, #0x3                ; =3
10022319c:      movk    x9, #0x7ffc, lsl #48
1002231a0:      cmp x26, x9
1002231a4:      b.eq    0x1002231fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1434>
1002231a8:      mov x9, #0x4                ; =4
1002231ac:      movk    x9, #0x7ffc, lsl #48
1002231b0:      cmp x26, x9
1002231b4:      b.ne    0x100223238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1470>
1002231b8:      ldr x1, [x19, #0x10]
1002231bc:      ldr x8, [x19]
1002231c0:      sub x8, x8, x1
1002231c4:      cmp x8, #0x3
1002231c8:      b.ls    0x100223580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17b8>
1002231cc:      ldr x8, [x19, #0x8]
1002231d0:      mov w9, #0x7274             ; =29300
1002231d4:      movk    w9, #0x6575, lsl #16
1002231d8:      str w9, [x8, x1]
1002231dc:      b   0x100222fa8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11e0>
1002231e0:      ldp x1, x2, [sp, #0x68]
1002231e4:      mov x0, x19
1002231e8:      bl  0x100213c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1002231ec:      mov w12, #0x756e            ; =30062
1002231f0:      movk    w12, #0x6c6c, lsl #16
1002231f4:      mov w13, #0x2c              ; =44
1002231f8:      b   0x100222fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11ec>
1002231fc:      ldr x1, [x19, #0x10]
100223200:      ldr x8, [x19]
100223204:      sub x8, x8, x1
100223208:      cmp x8, #0x4
10022320c:      b.ls    0x100223558 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1790>
100223210:      ldr x8, [x19, #0x8]
100223214:      add x8, x8, x1
100223218:      mov w9, #0x65               ; =101
10022321c:      strb    w9, [x8, #0x4]
100223220:      mov w9, #0x6166             ; =24934
100223224:      movk    w9, #0x736c, lsl #16
100223228:      str w9, [x8]
10022322c:      ldr x8, [x19, #0x10]
100223230:      add x8, x8, #0x5
100223234:      b   0x100222fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11e8>
100223238:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
10022323c:      cmp x8, x9
100223240:      b.eq    0x100223274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14ac>
100223244:      mov x9, #0x7ffa000000000000 ; =9221683186994511872
100223248:      cmp x8, x9
10022324c:      b.ne    0x1002232dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1514>
100223250:      str x22, [sp, #0x60]
100223254:      add x1, sp, #0x60
100223258:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
10022325c:      add x0, x0, #0x228
100223260:      bl  0x1001641ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100223264:      mov.16b v0, v8
100223268:      mov x0, x19
10022326c:      bl  0x100212e58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars16serialize_bigint>
100223270:      b   0x1002230e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x131c>
100223274:      str x22, [sp, #0x60]
100223278:      add x1, sp, #0x60
10022327c:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
100223280:      add x0, x0, #0x228
100223284:      bl  0x1001641ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100223288:      and x25, x26, #0xffffffffffff
10022328c:      cmp x25, #0x100, lsl #12    ; =0x100000
100223290:      b.hs    0x100223330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1568>
100223294:      ldr x1, [x19, #0x10]
100223298:      ldr x8, [x19]
10022329c:      sub x8, x8, x1
1002232a0:      cmp x8, #0x3
1002232a4:      mov w12, #0x756e            ; =30062
1002232a8:      movk    w12, #0x6c6c, lsl #16
1002232ac:      mov w13, #0x2c              ; =44
1002232b0:      b.hi    0x100222fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1002232b4:      mov x0, x19
1002232b8:      mov w2, #0x4                ; =4
1002232bc:      mov w3, #0x1                ; =1
1002232c0:      mov w4, #0x1                ; =1
1002232c4:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002232c8:      mov w13, #0x2c              ; =44
1002232cc:      mov w12, #0x756e            ; =30062
1002232d0:      movk    w12, #0x6c6c, lsl #16
1002232d4:      ldr x1, [x19, #0x10]
1002232d8:      b   0x100222fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11d8>
1002232dc:      lsr x8, x26, #52
1002232e0:      cmp x8, #0x0
1002232e4:      and x8, x26, #0x7
1002232e8:      ccmp    x26, #0x0, #0x4, eq
1002232ec:      ccmp    x8, #0x0, #0x0, ne
1002232f0:      b.eq    0x100223304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x153c>
1002232f4:      mov x0, x19
1002232f8:      mov.16b v0, v8
1002232fc:      bl  0x10021294c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100223300:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
100223304:      mov x0, x26
100223308:      bl  0x100226c64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify26ptr_is_tracked_heap_object>
10022330c:      tbz w0, #0x0, 0x1002232f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x152c>
100223310:      str x22, [sp, #0x60]
100223314:      add x1, sp, #0x60
100223318:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
10022331c:      add x0, x0, #0x228
100223320:      bl  0x1001641ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100223324:      mov x25, x26
100223328:      cmp x26, #0x100, lsl #12    ; =0x100000
10022332c:      b.lo    0x100223294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x14cc>
100223330:      mov x0, x26
100223334:      bl  0x100220b94 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify16is_closure_value>
100223338:      tbnz    w0, #0x0, 0x100223348 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1580>
10022333c:      mov x0, x26
100223340:      bl  0x10021fe78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify15is_symbol_value>
100223344:      cbz w0, 0x100223380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15b8>
100223348:      ldr x1, [x19, #0x10]
10022334c:      ldr x8, [x19]
100223350:      sub x8, x8, x1
100223354:      cmp x8, #0x3
100223358:      b.ls    0x100223640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1878>
10022335c:      ldr x8, [x19, #0x8]
100223360:      mov w12, #0x756e            ; =30062
100223364:      movk    w12, #0x6c6c, lsl #16
100223368:      str w12, [x8, x1]
10022336c:      ldr x8, [x19, #0x10]
100223370:      add x8, x8, #0x4
100223374:      str x8, [x19, #0x10]
100223378:      mov w13, #0x2c              ; =44
10022337c:      b   0x100222fb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x11ec>
100223380:      mov.16b v0, v8
100223384:      bl  0x1002492f4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins10formatting16boxed_primitives26boxed_primitive_json_value>
100223388:      cmp x0, #0x1
10022338c:      b.ne    0x1002233c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x15fc>
100223390:      mov.16b v9, v0
100223394:      mov x0, x25
100223398:      bl  0x1002216b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify18object_get_to_json>
10022339c:      tbz w0, #0x0, 0x1002233e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1618>
1002233a0:      mov.16b v8, v0
1002233a4:      bl  0x100226684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify24arm_to_json_result_guard>
1002233a8:      add w1, w20, #0x1
1002233ac:      mov.16b v0, v8
1002233b0:      mov x0, x19
1002233b4:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1002233b8:      ldr x8, [sp, #0x8]
1002233bc:      strb    wzr, [x8]
1002233c0:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
1002233c4:      mov x0, x25
1002233c8:      bl  0x1001eb7f4 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4date17is_date_cell_addr>
1002233cc:      tbz w0, #0x0, 0x1002233f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x162c>
1002233d0:      mov.16b v0, v8
1002233d4:      bl  0x1002d2d10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object17date_proto_thunks18date_to_json_value>
1002233d8:      add w1, w20, #0x1
1002233dc:      b   0x1002233e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1620>
1002233e0:      add w1, w20, #0x1
1002233e4:      mov.16b v0, v9
1002233e8:      mov x0, x19
1002233ec:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
1002233f0:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
1002233f4:      mov x0, x25
1002233f8:      bl  0x1002183d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json8raw_json19raw_json_text_bytes>
1002233fc:      cbz x0, 0x1002234d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1708>
100223400:      add x8, sp, #0x60
100223404:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100223408:      ldr w26, [sp, #0x60]
10022340c:      ldp x9, x8, [sp, #0x68]
100223410:      str x9, [sp]
100223414:      cmp w26, #0x0
100223418:      mov w9, #0x4                ; =4
10022341c:      csel    x25, x9, x8, ne
100223420:      ldr x1, [x19, #0x10]
100223424:      ldr x8, [x19]
100223428:      sub x8, x8, x1
10022342c:      cmp x25, x8
100223430:      b.hi    0x10022365c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1894>
100223434:      cbz x25, 0x100223464 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x169c>
100223438:      cmp w26, #0x0
10022343c:      adrp    x8, 0x100dba000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_USER_DEFINED+0x1ba>
100223440:      add x8, x8, #0x888
100223444:      ldr x9, [sp]
100223448:      csel    x8, x8, x9, ne
10022344c:      ldr x9, [x19, #0x8]
100223450:      add x0, x9, x1
100223454:      mov x1, x8
100223458:      mov x2, x25
10022345c:      bl  0x100cd8dac <_writev+0x100cd8dac>
100223460:      ldr x1, [x19, #0x10]
100223464:      add x8, x1, x25
100223468:      str x8, [x19, #0x10]
10022346c:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
100223470:      mov x0, x19
100223474:      mov x1, x25
100223478:      mov w2, #0x1                ; =1
10022347c:      mov w3, #0x1                ; =1
100223480:      mov w4, #0x1                ; =1
100223484:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223488:      mov w13, #0x2c              ; =44
10022348c:      mov w12, #0x756e            ; =30062
100223490:      movk    w12, #0x6c6c, lsl #16
100223494:      b   0x100222fd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1210>
100223498:      cmp w8, #0x2
10022349c:      b.eq    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
1002234a0:      mov x0, x21
1002234a4:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1002234a8:      add x1, x1, #0x824
1002234ac:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1002234b0:      strb    wzr, [x21, #0x20]
1002234b4:      mov w12, #0x756e            ; =30062
1002234b8:      movk    w12, #0x6c6c, lsl #16
1002234bc:      mov w13, #0x2c              ; =44
1002234c0:      ldr x8, [x21]
1002234c4:      cmp x8, x27
1002234c8:      b.lo    0x100223068 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x12a0>
1002234cc:      b   0x1002236d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1910>
1002234d0:      mov x0, x25
1002234d4:      bl  0x10022c620 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header20is_registered_buffer>
1002234d8:      tbz w0, #0x0, 0x1002234ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1724>
1002234dc:      mov x0, x25
1002234e0:      mov x1, x19
1002234e4:      bl  0x10020fccc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer16stringify_buffer>
1002234e8:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
1002234ec:      mov x0, x25
1002234f0:      bl  0x1001dbe18 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray23lookup_typed_array_kind>
1002234f4:      tbz w0, #0x0, 0x100223508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1740>
1002234f8:      mov x0, x25
1002234fc:      mov x1, x19
100223500:      bl  0x100210954 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json16stringify_buffer21stringify_typed_array>
100223504:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
100223508:      lsr x8, x25, #47
10022350c:      cbnz    x8, 0x1002235ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1824>
100223510:      ldurb   w8, [x25, #-0x8]
100223514:      cmp w8, #0x4
100223518:      b.gt    0x1002235a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17e0>
10022351c:      cmp w8, #0x1
100223520:      b.eq    0x100223624 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x185c>
100223524:      cmp w8, #0x2
100223528:      b.eq    0x1002235f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1830>
10022352c:      cmp w8, #0x3
100223530:      b.ne    0x1002235ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1824>
100223534:      ldr w2, [x25, #0x4]
100223538:      b   0x100223638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1870>
10022353c:      mov x0, x19
100223540:      mov w2, #0x4                ; =4
100223544:      mov w3, #0x1                ; =1
100223548:      mov w4, #0x1                ; =1
10022354c:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223550:      ldr x1, [x19, #0x10]
100223554:      b   0x100223178 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x13b0>
100223558:      mov x0, x19
10022355c:      mov w2, #0x5                ; =5
100223560:      mov w3, #0x1                ; =1
100223564:      mov w4, #0x1                ; =1
100223568:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
10022356c:      mov w13, #0x2c              ; =44
100223570:      mov w12, #0x756e            ; =30062
100223574:      movk    w12, #0x6c6c, lsl #16
100223578:      ldr x1, [x19, #0x10]
10022357c:      b   0x100223210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1448>
100223580:      mov x0, x19
100223584:      mov w2, #0x4                ; =4
100223588:      mov w3, #0x1                ; =1
10022358c:      mov w4, #0x1                ; =1
100223590:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223594:      mov w13, #0x2c              ; =44
100223598:      mov w12, #0x756e            ; =30062
10022359c:      movk    w12, #0x6c6c, lsl #16
1002235a0:      ldr x1, [x19, #0x10]
1002235a4:      b   0x1002231cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1404>
1002235a8:      cmp w8, #0x5
1002235ac:      b.eq    0x1002235c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17f8>
1002235b0:      cmp w8, #0x8
1002235b4:      b.eq    0x1002235c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x17f8>
1002235b8:      cmp w8, #0xc
1002235bc:      b.ne    0x1002235ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1824>
1002235c0:      ldr x1, [x19, #0x10]
1002235c4:      ldr x8, [x19]
1002235c8:      sub x8, x8, x1
1002235cc:      cmp x8, #0x1
1002235d0:      b.ls    0x100223678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18b0>
1002235d4:      ldr x8, [x19, #0x8]
1002235d8:      mov w9, #0x7d7b             ; =32123
1002235dc:      strh    w9, [x8, x1]
1002235e0:      ldr x8, [x19, #0x10]
1002235e4:      add x8, x8, #0x2
1002235e8:      b   0x100223468 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x16a0>
1002235ec:      mov x0, x25
1002235f0:      bl  0x1002214c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify17is_object_pointer>
1002235f4:      tbz w0, #0x0, 0x10022360c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1844>
1002235f8:      add w2, w20, #0x1
1002235fc:      mov x0, x25
100223600:      mov x1, x19
100223604:      bl  0x100224c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify22stringify_object_inner>
100223608:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
10022360c:      ldp w8, w2, [x25]
100223610:      sub w9, w2, #0x1
100223614:      mov w10, #0x270f            ; =9999
100223618:      cmp w9, w10
10022361c:      ccmp    w8, w2, #0x2, lo
100223620:      b.hi    0x100223638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1870>
100223624:      add w2, w20, #0x1
100223628:      mov x0, x25
10022362c:      mov x1, x19
100223630:      bl  0x100221dc8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth>
100223634:      b   0x1002231ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1424>
100223638:      add x1, x25, #0x14
10022363c:      b   0x1002231e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x141c>
100223640:      mov x0, x19
100223644:      mov w2, #0x4                ; =4
100223648:      mov w3, #0x1                ; =1
10022364c:      mov w4, #0x1                ; =1
100223650:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223654:      ldr x1, [x19, #0x10]
100223658:      b   0x10022335c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1594>
10022365c:      mov x0, x19
100223660:      mov x2, x25
100223664:      mov w3, #0x1                ; =1
100223668:      mov w4, #0x1                ; =1
10022366c:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223670:      ldr x1, [x19, #0x10]
100223674:      b   0x100223438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1670>
100223678:      mov x0, x19
10022367c:      mov w2, #0x2                ; =2
100223680:      mov w3, #0x1                ; =1
100223684:      mov w4, #0x1                ; =1
100223688:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
10022368c:      ldr x1, [x19, #0x10]
100223690:      b   0x1002235d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x180c>
100223694:      ldr x21, [x19, #0x10]
100223698:      ldr x8, [x19]
10022369c:      cmp x8, x21
1002236a0:      b.eq    0x100223dd4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x200c>
1002236a4:      ldr x8, [x19, #0x8]
1002236a8:      mov w9, #0x5d               ; =93
1002236ac:      strb    w9, [x8, x21]
1002236b0:      add x8, x21, #0x1
1002236b4:      str x8, [x19, #0x10]
1002236b8:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
1002236bc:      add x0, x0, #0x2d8
1002236c0:      bl  0x100137fe8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths6_0INtNtBZ_6option6OptionjEEB2j_>
1002236c4:      add x0, sp, #0x30
1002236c8:      bl  0x1001c1380 <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueINtNtB4_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template13ShapeTemplateEEB13_>
1002236cc:      add x0, sp, #0x20
1002236d0:      bl  0x1001c15ac <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1002236d4:      b   0x100222188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3c0>
1002236d8:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1002236dc:      add x0, x0, #0xf70
1002236e0:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1002236e4:      adrp    x0, 0x100db5000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2120>
1002236e8:      add x0, x0, #0xd60
1002236ec:      mov w1, #0xb                ; =11
1002236f0:      bl  0x100cac2e4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1002236f4:      ldr x8, [x19]
1002236f8:      sub x8, x8, x20
1002236fc:      cmp x8, #0x3
100223700:      b.ls    0x100223e30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2068>
100223704:      ldr x8, [x19, #0x8]
100223708:      mov w9, #0x7274             ; =29300
10022370c:      movk    w9, #0x6575, lsl #16
100223710:      b   0x1002237dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a14>
100223714:      and x8, x0, #0xffff000000000000
100223718:      mov x9, #0x7fff000000000000 ; =9223090561878065152
10022371c:      cmp x8, x9
100223720:      b.eq    0x1002237b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19ec>
100223724:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100223728:      cmp x8, x9
10022372c:      b.ne    0x100223a04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c3c>
100223730:      strb    wzr, [sp, #0x64]
100223734:      str wzr, [sp, #0x60]
100223738:      add x1, sp, #0x60
10022373c:      bl  0x1001d1014 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
100223740:      mov x1, x0
100223744:      add x8, sp, #0x30
100223748:      add x0, sp, #0x60
10022374c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100223750:      ldr w8, [sp, #0x30]
100223754:      tbz w8, #0x0, 0x100223a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c54>
100223758:      ldr x1, [x19, #0x10]
10022375c:      ldr x8, [x19]
100223760:      sub x8, x8, x1
100223764:      cmp x8, #0x3
100223768:      b.ls    0x100223e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x20ac>
10022376c:      ldr x8, [x19, #0x8]
100223770:      mov w9, #0x756e             ; =30062
100223774:      movk    w9, #0x6c6c, lsl #16
100223778:      str w9, [x8, x1]
10022377c:      b   0x1002237e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a18>
100223780:      adrp    x0, 0x101093000 <_anon.438b28c8644b10f28676d307896bf03a.683>
100223784:      add x0, x0, #0x5c0
100223788:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10022378c:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
100223790:      add x0, x0, #0x498
100223794:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100223798:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10022379c:      add x0, x0, #0x4b0
1002237a0:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1002237a4:      adrp    x0, 0x100db5000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2120>
1002237a8:      add x0, x0, #0xde9
1002237ac:      mov w1, #0xf                ; =15
1002237b0:      bl  0x100cac2e4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1002237b4:      and x8, x0, #0xffffffffffff
1002237b8:      cmp x8, #0x1, lsl #12       ; =0x1000
1002237bc:      b.hs    0x100223a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c5c>
1002237c0:      ldr x8, [x19]
1002237c4:      sub x8, x8, x20
1002237c8:      cmp x8, #0x3
1002237cc:      b.ls    0x100223df0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x2028>
1002237d0:      ldr x8, [x19, #0x8]
1002237d4:      mov w9, #0x756e             ; =30062
1002237d8:      movk    w9, #0x6c6c, lsl #16
1002237dc:      str w9, [x8, x20]
1002237e0:      ldr x8, [x19, #0x10]
1002237e4:      add x8, x8, #0x4
1002237e8:      str x8, [x19, #0x10]
1002237ec:      cmp w21, #0x1
1002237f0:      b.ne    0x100223818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a50>
1002237f4:      ldr x20, [x19, #0x10]
1002237f8:      ldr x8, [x19]
1002237fc:      cmp x8, x20
100223800:      b.eq    0x100223d5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f94>
100223804:      ldr x8, [x19, #0x8]
100223808:      mov w9, #0x5d               ; =93
10022380c:      strb    w9, [x8, x20]
100223810:      add x8, x20, #0x1
100223814:      b   0x100222184 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3bc>
100223818:      add x21, x22, #0x10
10022381c:      mov w22, #0x756e            ; =30062
100223820:      movk    w22, #0x6c6c, lsl #16
100223824:      sub x23, x23, #0x8
100223828:      mov w25, #0x2c              ; =44
10022382c:      mov x26, #-0x7ffc000000000001 ; =-9222246136947933185
100223830:      mov x27, #0x3               ; =3
100223834:      movk    x27, #0x7ffc, lsl #48
100223838:      mov x28, #0x4               ; =4
10022383c:      movk    x28, #0x7ffc, lsl #48
100223840:      mov x24, #0x7ff9000000000000 ; =9221401712017801216
100223844:      b   0x100223878 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ab0>
100223848:      ldr x1, [x19, #0x10]
10022384c:      ldr x8, [x19]
100223850:      sub x8, x8, x1
100223854:      cmp x8, #0x3
100223858:      b.ls    0x1002239b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1be8>
10022385c:      ldr x8, [x19, #0x8]
100223860:      str w22, [x8, x1]
100223864:      ldr x8, [x19, #0x10]
100223868:      add x8, x8, #0x4
10022386c:      str x8, [x19, #0x10]
100223870:      subs    x23, x23, #0x8
100223874:      b.eq    0x1002237f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a2c>
100223878:      ldr d0, [x21], #0x8
10022387c:      ldr x20, [x19, #0x10]
100223880:      ldr x8, [x19]
100223884:      cmp x8, x20
100223888:      b.eq    0x10022398c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bc4>
10022388c:      fmov    x0, d0
100223890:      ldr x8, [x19, #0x8]
100223894:      strb    w25, [x8, x20]
100223898:      add x1, x20, #0x1
10022389c:      str x1, [x19, #0x10]
1002238a0:      add x8, x0, x26
1002238a4:      cmp x8, #0x2
1002238a8:      b.lo    0x10022384c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a84>
1002238ac:      cmp x0, x27
1002238b0:      b.eq    0x1002238e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b18>
1002238b4:      cmp x0, x28
1002238b8:      b.ne    0x100223918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b50>
1002238bc:      ldr x8, [x19]
1002238c0:      sub x8, x8, x1
1002238c4:      cmp x8, #0x3
1002238c8:      b.ls    0x1002239cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c04>
1002238cc:      ldr x8, [x19, #0x8]
1002238d0:      mov w9, #0x7274             ; =29300
1002238d4:      movk    w9, #0x6575, lsl #16
1002238d8:      str w9, [x8, x1]
1002238dc:      b   0x100223864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a9c>
1002238e0:      ldr x8, [x19]
1002238e4:      sub x8, x8, x1
1002238e8:      cmp x8, #0x4
1002238ec:      b.ls    0x1002239e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c20>
1002238f0:      ldr x8, [x19, #0x8]
1002238f4:      add x8, x8, x1
1002238f8:      mov w9, #0x65               ; =101
1002238fc:      strb    w9, [x8, #0x4]
100223900:      mov w9, #0x6166             ; =24934
100223904:      movk    w9, #0x736c, lsl #16
100223908:      str w9, [x8]
10022390c:      ldr x8, [x19, #0x10]
100223910:      add x8, x8, #0x5
100223914:      b   0x10022386c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa4>
100223918:      and x8, x0, #0xffff000000000000
10022391c:      cmp x8, x24
100223920:      b.eq    0x100223948 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b80>
100223924:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100223928:      cmp x8, x9
10022392c:      b.ne    0x100223980 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bb8>
100223930:      and x8, x0, #0xffffffffffff
100223934:      cmp x8, #0x1, lsl #12       ; =0x1000
100223938:      b.lo    0x10022384c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a84>
10022393c:      ldr w2, [x8, #0x4]
100223940:      add x1, x8, #0x14
100223944:      b   0x100223974 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1bac>
100223948:      strb    wzr, [sp, #0x64]
10022394c:      str wzr, [sp, #0x60]
100223950:      add x1, sp, #0x60
100223954:      bl  0x1001d1014 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime5value7jsvalueNtB2_7JSValue19short_string_to_buf>
100223958:      mov x1, x0
10022395c:      add x8, sp, #0x30
100223960:      add x0, sp, #0x60
100223964:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100223968:      ldr w8, [sp, #0x30]
10022396c:      tbnz    w8, #0x0, 0x100223848 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a80>
100223970:      ldp x1, x2, [sp, #0x38]
100223974:      mov x0, x19
100223978:      bl  0x100213c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
10022397c:      b   0x100223870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa8>
100223980:      mov x0, x19
100223984:      bl  0x10021294c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100223988:      b   0x100223870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1aa8>
10022398c:      mov x0, x19
100223990:      mov x1, x20
100223994:      mov w2, #0x1                ; =1
100223998:      mov w3, #0x1                ; =1
10022399c:      mov w4, #0x1                ; =1
1002239a0:      mov.16b v8, v0
1002239a4:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002239a8:      mov.16b v0, v8
1002239ac:      b   0x10022388c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ac4>
1002239b0:      mov x0, x19
1002239b4:      mov w2, #0x4                ; =4
1002239b8:      mov w3, #0x1                ; =1
1002239bc:      mov w4, #0x1                ; =1
1002239c0:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002239c4:      ldr x1, [x19, #0x10]
1002239c8:      b   0x10022385c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a94>
1002239cc:      mov x0, x19
1002239d0:      mov w2, #0x4                ; =4
1002239d4:      mov w3, #0x1                ; =1
1002239d8:      mov w4, #0x1                ; =1
1002239dc:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002239e0:      ldr x1, [x19, #0x10]
1002239e4:      b   0x1002238cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b04>
1002239e8:      mov x0, x19
1002239ec:      mov w2, #0x5                ; =5
1002239f0:      mov w3, #0x1                ; =1
1002239f4:      mov w4, #0x1                ; =1
1002239f8:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1002239fc:      ldr x1, [x19, #0x10]
100223a00:      b   0x1002238f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1b28>
100223a04:      mov x0, x19
100223a08:      bl  0x10021294c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars12write_number>
100223a0c:      b   0x1002237ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a24>
100223a10:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223a14:      add x0, x0, #0xd78
100223a18:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100223a1c:      ldp x1, x2, [sp, #0x38]
100223a20:      b   0x100223a2c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1c64>
100223a24:      ldr w2, [x8, #0x4]
100223a28:      add x1, x8, #0x14
100223a2c:      mov x0, x19
100223a30:      bl  0x100213c58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
100223a34:      b   0x1002237ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a24>
100223a38:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223a3c:      add x0, x0, #0xd90
100223a40:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100223a44:      adrp    x0, 0x100db7000 <_anon.d22baf9b4aae6fad60dab30783929d4b.232+0x8d8>
100223a48:      add x0, x0, #0xbf3
100223a4c:      mov w1, #0xf                ; =15
100223a50:      bl  0x100cac2e4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100223a54:      mov x0, x19
100223a58:      mov x1, x23
100223a5c:      mov w2, #0x1                ; =1
100223a60:      mov w3, #0x1                ; =1
100223a64:      mov w4, #0x1                ; =1
100223a68:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223a6c:      b   0x1002223f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x62c>
100223a70:      mov x0, x19
100223a74:      mov x1, x23
100223a78:      mov w2, #0x1                ; =1
100223a7c:      mov w3, #0x1                ; =1
100223a80:      mov w4, #0x1                ; =1
100223a84:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223a88:      b   0x100222af4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xd2c>
100223a8c:      cmp w8, #0x1
100223a90:      b.ne    0x100223ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1cfc>
100223a94:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100223a98:      add x1, x1, #0x824
100223a9c:      mov x0, x24
100223aa0:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100223aa4:      strb    wzr, [x24, #0x20]
100223aa8:      ldr x8, [x24]
100223aac:      cbz x8, 0x1002222e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x51c>
100223ab0:      adrp    x0, 0x101093000 <_anon.438b28c8644b10f28676d307896bf03a.683>
100223ab4:      add x0, x0, #0x8c0
100223ab8:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100223abc:      cmp w8, #0x2
100223ac0:      b.ne    0x100223d18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f50>
100223ac4:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100223ac8:      add x0, x0, #0xed8
100223acc:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100223ad0:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223ad4:      add x0, x0, #0xc30
100223ad8:      add x1, sp, #0x28
100223adc:      bl  0x100135650 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100223ae0:      ldr d8, [x0, #0x8]
100223ae4:      fmov    x0, d8
100223ae8:      add x1, sp, #0x30
100223aec:      add w3, w20, #0x1
100223af0:      mov x2, x19
100223af4:      bl  0x100215e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100223af8:      cbnz    w0, 0x100223b0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d44>
100223afc:      add w1, w20, #0x1
100223b00:      mov.16b v0, v8
100223b04:      mov x0, x19
100223b08:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100223b0c:      mov w8, #0x1                ; =1
100223b10:      ldr x9, [sp, #0x10]
100223b14:      sub x25, x8, x9
100223b18:      mov w26, #0x2               ; =2
100223b1c:      mov w27, #0x2c              ; =44
100223b20:      adrp    x22, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
100223b24:      add x22, x22, #0x228
100223b28:      adrp    x23, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223b2c:      add x23, x23, #0xc30
100223b30:      b   0x100223b44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d7c>
100223b34:      add x26, x26, #0x1
100223b38:      add x8, x25, x26
100223b3c:      cmp x8, #0x2
100223b40:      b.eq    0x100223c60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e98>
100223b44:      ldr x24, [x19, #0x10]
100223b48:      ldr x8, [x19]
100223b4c:      cmp x8, x24
100223b50:      b.eq    0x100223c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e5c>
100223b54:      sub x8, x26, #0x1
100223b58:      ldr x9, [x19, #0x8]
100223b5c:      strb    w27, [x9, x24]
100223b60:      add x9, x24, #0x1
100223b64:      str x9, [x19, #0x10]
100223b68:      str x8, [sp, #0x60]
100223b6c:      add x1, sp, #0x60
100223b70:      mov x0, x22
100223b74:      bl  0x1001641ec <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellNtNtCsctvjasLqLe9_5alloc6string6StringEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe21set_to_json_key_index0uEB2m_>
100223b78:      ldr x8, [x28]
100223b7c:      cmn x8, #0x1
100223b80:      b.eq    0x100223be8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e20>
100223b84:      mrs x9, TPIDRRO_EL0
100223b88:      and x9, x9, #0xfffffffffffffff8
100223b8c:      ldr x8, [x9, x8, lsl #3]
100223b90:      cbz x8, 0x100223be8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e20>
100223b94:      ldr x8, [x8, #0x19e8]
100223b98:      cbz x8, 0x100223be8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e20>
100223b9c:      ldr x9, [x8]
100223ba0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100223ba4:      cmp x9, x10
100223ba8:      b.hs    0x100223c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e7c>
100223bac:      ldr x10, [sp, #0x28]
100223bb0:      add x11, x9, #0x1
100223bb4:      str x11, [x8]
100223bb8:      ldr x11, [x8, #0x18]
100223bbc:      cmp x10, x11
100223bc0:      b.hs    0x100223c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e78>
100223bc4:      ldr x11, [x8, #0x10]
100223bc8:      mov w12, #0x18              ; =24
100223bcc:      madd    x10, x10, x12, x11
100223bd0:      ldr x11, [x10]
100223bd4:      cmp x11, #0x1
100223bd8:      b.ne    0x100223c50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e88>
100223bdc:      ldr x0, [x10, #0x8]
100223be0:      str x9, [x8]
100223be4:      b   0x100223bf4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1e2c>
100223be8:      add x1, sp, #0x28
100223bec:      mov x0, x23
100223bf0:      bl  0x100135650 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100223bf4:      ldr d8, [x0, x26, lsl #3]
100223bf8:      fmov    x0, d8
100223bfc:      add x1, sp, #0x30
100223c00:      add w3, w20, #0x1
100223c04:      mov x2, x19
100223c08:      bl  0x100215e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_shape_template22try_emit_shape_element>
100223c0c:      tbnz    w0, #0x0, 0x100223b34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d6c>
100223c10:      add w1, w20, #0x1
100223c14:      mov.16b v0, v8
100223c18:      mov x0, x19
100223c1c:      bl  0x100223edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_value_depth>
100223c20:      b   0x100223b34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d6c>
100223c24:      mov x0, x19
100223c28:      mov x1, x24
100223c2c:      mov w2, #0x1                ; =1
100223c30:      mov w3, #0x1                ; =1
100223c34:      mov w4, #0x1                ; =1
100223c38:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223c3c:      b   0x100223b54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1d8c>
100223c40:      bl  0x100cac31c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100223c44:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223c48:      add x0, x0, #0xd18
100223c4c:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100223c50:      adrp    x0, 0x100db7000 <_anon.d22baf9b4aae6fad60dab30783929d4b.232+0x8d8>
100223c54:      add x0, x0, #0xb7f
100223c58:      mov w1, #0xb                ; =11
100223c5c:      bl  0x100cac2e4 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100223c60:      ldr x20, [x19, #0x10]
100223c64:      ldr x8, [x19]
100223c68:      cmp x8, x20
100223c6c:      b.eq    0x100223eac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x20e4>
100223c70:      ldr x8, [x19, #0x8]
100223c74:      mov w9, #0x5d               ; =93
100223c78:      strb    w9, [x8, x20]
100223c7c:      add x8, x20, #0x1
100223c80:      str x8, [x19, #0x10]
100223c84:      adrp    x0, 0x101098000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.133+0x10>
100223c88:      add x0, x0, #0x2d8
100223c8c:      bl  0x100137f78 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecjEEE4withNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depths4_0INtNtBZ_6option6OptionjEEB2j_>
100223c90:      ldr x8, [sp, #0x30]
100223c94:      cmn x8, #0x1
100223c98:      b.eq    0x100223cb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1eec>
100223c9c:      add x0, sp, #0x30
100223ca0:      bl  0x100a17628 <__RNvXsp_NtCsctvjasLqLe9_5alloc3vecINtB5_3VecNtNtCs8BpVhDwHqJW_3std4path7PathBufENtNtNtCsjgY6bXVaRmE_4core3ops4drop4Drop4dropCs5gMwpk3Cs4e_13perry_runtime>
100223ca4:      ldr x8, [sp, #0x30]
100223ca8:      cbz x8, 0x100223cb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1eec>
100223cac:      ldr x0, [sp, #0x38]
100223cb0:      bl  0x100cd5f00 <_mi_free>
100223cb4:      ldr x8, [x28]
100223cb8:      cmn x8, #0x1
100223cbc:      b.eq    0x100223d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f3c>
100223cc0:      mrs x9, TPIDRRO_EL0
100223cc4:      and x9, x9, #0xfffffffffffffff8
100223cc8:      ldr x8, [x9, x8, lsl #3]
100223ccc:      cbz x8, 0x100223d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f3c>
100223cd0:      ldr x8, [x8, #0x19e8]
100223cd4:      cbz x8, 0x100223d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f3c>
100223cd8:      ldr x9, [x8]
100223cdc:      cbnz    x9, 0x100223cf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f30>
100223ce0:      ldr x9, [x8, #0x18]
100223ce4:      cmp x21, x9
100223ce8:      b.hi    0x100223cf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1f28>
100223cec:      str x21, [x8, #0x18]
100223cf0:      str xzr, [x8]
100223cf4:      b   0x100222188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3c0>
100223cf8:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223cfc:      add x0, x0, #0xec8
100223d00:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100223d04:      adrp    x0, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223d08:      add x0, x0, #0xc30
100223d0c:      add x1, sp, #0x20
100223d10:      bl  0x100135c08 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100223d14:      b   0x100222188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x3c0>
100223d18:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
100223d1c:      add x1, x1, #0x824
100223d20:      mov x0, x24
100223d24:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100223d28:      strb    wzr, [x24, #0x20]
100223d2c:      ldr x8, [x24]
100223d30:      cbz x8, 0x100222b6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xda4>
100223d34:      adrp    x0, 0x101093000 <_anon.438b28c8644b10f28676d307896bf03a.683>
100223d38:      add x0, x0, #0x8d8
100223d3c:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100223d40:      mov x0, x19
100223d44:      mov x1, x20
100223d48:      mov w2, #0x1                ; =1
100223d4c:      mov w3, #0x1                ; =1
100223d50:      mov w4, #0x1                ; =1
100223d54:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223d58:      b   0x100222c20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xe58>
100223d5c:      mov x0, x19
100223d60:      mov x1, x20
100223d64:      mov w2, #0x1                ; =1
100223d68:      mov w3, #0x1                ; =1
100223d6c:      mov w4, #0x1                ; =1
100223d70:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223d74:      b   0x100223804 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a3c>
100223d78:      adrp    x0, 0x100dbb000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.729+0x62>
100223d7c:      add x0, x0, #0x656
100223d80:      mov w1, #0x34               ; =52
100223d84:      bl  0x1004722b8 <_js_string_from_bytes>
100223d88:      bl  0x10026cb50 <_js_rangeerror_new>
100223d8c:      mov x8, #0x1                ; =1
100223d90:      movk    x8, #0x7ffc, lsl #48
100223d94:      lsr x9, x0, #52
100223d98:      mov x10, #0x7ffd000000000000 ; =9222527611924643840
100223d9c:      bfxil   x10, x0, #0, #48
100223da0:      cmp x9, #0x7fe
100223da4:      csel    x9, x0, x10, hi
100223da8:      cmp x0, #0x0
100223dac:      csinc   x8, x9, x8, ne
100223db0:      fmov    d0, x8
100223db4:      bl  0x1006bcb44 <_js_throw>
100223db8:      mov x0, x19
100223dbc:      mov x1, x21
100223dc0:      mov w2, #0x1                ; =1
100223dc4:      mov w3, #0x1                ; =1
100223dc8:      mov w4, #0x1                ; =1
100223dcc:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223dd0:      b   0x100222f24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x115c>
100223dd4:      mov x0, x19
100223dd8:      mov x1, x21
100223ddc:      mov w2, #0x1                ; =1
100223de0:      mov w3, #0x1                ; =1
100223de4:      mov w4, #0x1                ; =1
100223de8:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223dec:      b   0x1002236a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x18dc>
100223df0:      mov x0, x19
100223df4:      mov x1, x20
100223df8:      mov w2, #0x4                ; =4
100223dfc:      mov w3, #0x1                ; =1
100223e00:      mov w4, #0x1                ; =1
100223e04:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223e08:      ldr x20, [x19, #0x10]
100223e0c:      b   0x1002237d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1a08>
100223e10:      mov x0, x19
100223e14:      mov x1, x20
100223e18:      mov w2, #0x5                ; =5
100223e1c:      mov w3, #0x1                ; =1
100223e20:      mov w4, #0x1                ; =1
100223e24:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223e28:      ldr x20, [x19, #0x10]
100223e2c:      b   0x100222c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0xec0>
100223e30:      mov x0, x19
100223e34:      mov x1, x20
100223e38:      mov w2, #0x4                ; =4
100223e3c:      mov w3, #0x1                ; =1
100223e40:      mov w4, #0x1                ; =1
100223e44:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223e48:      ldr x20, [x19, #0x10]
100223e4c:      b   0x100223704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x193c>
100223e50:      adrp    x0, 0x100dbb000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.729+0x62>
100223e54:      add x0, x0, #0x631
100223e58:      mov w1, #0x25               ; =37
100223e5c:      bl  0x1004722b8 <_js_string_from_bytes>
100223e60:      bl  0x10027baa8 <_js_typeerror_new>
100223e64:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100223e68:      bfxil   x8, x0, #0, #48
100223e6c:      fmov    d0, x8
100223e70:      bl  0x1006bcb44 <_js_throw>
100223e74:      mov x0, x19
100223e78:      mov w2, #0x4                ; =4
100223e7c:      mov w3, #0x1                ; =1
100223e80:      mov w4, #0x1                ; =1
100223e84:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223e88:      ldr x1, [x19, #0x10]
100223e8c:      b   0x10022376c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x19a4>
100223e90:      mov x0, x19
100223e94:      mov x1, x22
100223e98:      mov w2, #0x1                ; =1
100223e9c:      mov w3, #0x1                ; =1
100223ea0:      mov w4, #0x1                ; =1
100223ea4:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223ea8:      b   0x100222e34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x106c>
100223eac:      mov x0, x19
100223eb0:      mov x1, x20
100223eb4:      mov w2, #0x1                ; =1
100223eb8:      mov w3, #0x1                ; =1
100223ebc:      mov w4, #0x1                ; =1
100223ec0:      bl  0x100cb6cc0 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100223ec4:      b   0x100223c70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9stringify21stringify_array_depth+0x1ea8>
100223ec8:      adrp    x2, 0x101097000 <_anon.d22baf9b4aae6fad60dab30783929d4b.246+0xb38>
100223ecc:      add x2, x2, #0xf90
100223ed0:      mov w0, #0x5                ; =5
100223ed4:      mov w1, #0x5                ; =5
100223ed8:      bl  0x100c8d30c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
