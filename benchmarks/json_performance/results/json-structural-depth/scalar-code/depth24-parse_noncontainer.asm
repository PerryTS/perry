/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/depth24-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004d1f34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>:
1004d1f34:      sub sp, sp, #0x90
1004d1f38:      stp d9, d8, [sp, #0x20]
1004d1f3c:      stp x28, x27, [sp, #0x30]
1004d1f40:      stp x26, x25, [sp, #0x40]
1004d1f44:      stp x24, x23, [sp, #0x50]
1004d1f48:      stp x22, x21, [sp, #0x60]
1004d1f4c:      stp x20, x19, [sp, #0x70]
1004d1f50:      stp x29, x30, [sp, #0x80]
1004d1f54:      add x29, sp, #0x80
1004d1f58:      ldrb    w9, [x0, #0x14]
1004d1f5c:      orr w8, w9, #0x20
1004d1f60:      cmp w8, #0x7b
1004d1f64:      b.ne    0x1004d1f8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x58>
1004d1f68:      ldp x29, x30, [sp, #0x80]
1004d1f6c:      ldp x20, x19, [sp, #0x70]
1004d1f70:      ldp x22, x21, [sp, #0x60]
1004d1f74:      ldp x24, x23, [sp, #0x50]
1004d1f78:      ldp x26, x25, [sp, #0x40]
1004d1f7c:      ldp x28, x27, [sp, #0x30]
1004d1f80:      ldp d9, d8, [sp, #0x20]
1004d1f84:      add sp, sp, #0x90
1004d1f88:      b   0x1004cf9f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1004d1f8c:      mov x26, #0x0               ; =0
1004d1f90:      sub x28, x1, #0x2
1004d1f94:      mov x12, #-0x20000000000    ; =-2199023255552
1004d1f98:      sub x10, x1, #0x1
1004d1f9c:      mov x24, #-0x15             ; =-21
1004d1fa0:      mov x8, #0x2600             ; =9728
1004d1fa4:      movk    x8, #0x1, lsl #32
1004d1fa8:      mov x11, #-0x10000000000    ; =-1099511627776
1004d1fac:      sub x25, x1, #0x2
1004d1fb0:      add x21, x12, x1, lsl #40
1004d1fb4:      cmp w9, #0x20
1004d1fb8:      b.hi    0x1004d1ff0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d1fbc:      mov w12, w9
1004d1fc0:      lsr x12, x8, x12
1004d1fc4:      tbz w12, #0x0, 0x1004d1ff0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d1fc8:      cmp x10, x26
1004d1fcc:      b.eq    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d1fd0:      add x9, x0, x26
1004d1fd4:      ldrb    w9, [x9, #0x15]
1004d1fd8:      sub x25, x25, #0x1
1004d1fdc:      add x26, x26, #0x1
1004d1fe0:      add x21, x21, x11
1004d1fe4:      sub x24, x24, #0x1
1004d1fe8:      cmp w9, #0x20
1004d1fec:      b.ls    0x1004d1fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x88>
1004d1ff0:      add x23, x0, #0x13
1004d1ff4:      add x11, x0, x26
1004d1ff8:      mov x8, #0x2600             ; =9728
1004d1ffc:      movk    x8, #0x1, lsl #32
1004d2000:      mov x13, #-0x10000000000    ; =-1099511627776
1004d2004:      mov x22, x26
1004d2008:      ldrb    w12, [x23, x1]
1004d200c:      cmp w12, #0x20
1004d2010:      b.hi    0x1004d2038 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x104>
1004d2014:      lsr x14, x8, x12
1004d2018:      tbz w14, #0x0, 0x1004d2038 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x104>
1004d201c:      sub x23, x23, #0x1
1004d2020:      add x22, x22, #0x1
1004d2024:      add x21, x21, x13
1004d2028:      sub x25, x25, #0x1
1004d202c:      cmp x1, x22
1004d2030:      b.ne    0x1004d2008 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xd4>
1004d2034:      b   0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2038:      sub x8, x1, x22
1004d203c:      cmp x8, #0x4
1004d2040:      b.eq    0x1004d20ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x178>
1004d2044:      cmp x8, #0x5
1004d2048:      b.ne    0x1004d20b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d204c:      cmp w9, #0x22
1004d2050:      b.eq    0x1004d21b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x280>
1004d2054:      cmp w9, #0x2d
1004d2058:      b.eq    0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x19c>
1004d205c:      cmp w9, #0x66
1004d2060:      b.ne    0x1004d20c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x190>
1004d2064:      add x8, x0, x26
1004d2068:      ldrb    w9, [x8, #0x15]
1004d206c:      cmp w9, #0x61
1004d2070:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2074:      ldrb    w8, [x8, #0x16]
1004d2078:      cmp w8, #0x6c
1004d207c:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2080:      add x8, x0, x26
1004d2084:      ldrb    w9, [x8, #0x17]
1004d2088:      cmp w9, #0x73
1004d208c:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2090:      ldrb    w8, [x8, #0x18]
1004d2094:      cmp w8, #0x65
1004d2098:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d209c:      mov x8, #0x2                ; =2
1004d20a0:      movk    x8, #0x7ffc, lsl #48
1004d20a4:      orr x19, x8, #0x1
1004d20a8:      b   0x1004d20fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d20ac:      cmp w9, #0x6d
1004d20b0:      b.gt    0x1004d2214 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2e0>
1004d20b4:      cmp w9, #0x22
1004d20b8:      b.eq    0x1004d21b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x280>
1004d20bc:      cmp w9, #0x2d
1004d20c0:      b.eq    0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x19c>
1004d20c4:      sub w9, w9, #0x30
1004d20c8:      cmp w9, #0xa
1004d20cc:      b.hs    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d20d0:      mov x19, x0
1004d20d4:      add x0, x11, #0x14
1004d20d8:      mov x20, x1
1004d20dc:      mov x1, x8
1004d20e0:      bl  0x1004bc148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004d20e4:      mov x8, x0
1004d20e8:      mov x0, x19
1004d20ec:      mov x19, x1
1004d20f0:      mov x1, x20
1004d20f4:      cmp x8, #0x1
1004d20f8:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d20fc:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2100:      add x0, x0, #0x9d0
1004d2104:      ldr x8, [x0]
1004d2108:      blr x8
1004d210c:      ldrb    w9, [x0]
1004d2110:      strb    wzr, [x0]
1004d2114:      cbz w9, 0x1004d2154 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x220>
1004d2118:      mov x8, x0
1004d211c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2120:      add x0, x0, #0x9e8
1004d2124:      ldr x9, [x0]
1004d2128:      blr x9
1004d212c:      ldrb    w9, [x0]
1004d2130:      tst w9, #0x3
1004d2134:      b.ne    0x1004d214c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x218>
1004d2138:      adrp    x9, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004d213c:      add x9, x9, #0x8fc
1004d2140:      ldapr   w9, [x9]
1004d2144:      cmp w9, #0x0
1004d2148:      b.le    0x1004d22d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x39c>
1004d214c:      mov w9, #0x1                ; =1
1004d2150:      strb    w9, [x8]
1004d2154:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004d2158:      add x0, x0, #0xb60
1004d215c:      ldr x8, [x0]
1004d2160:      blr x8
1004d2164:      ldrb    w8, [x0, #0x38]
1004d2168:      cmp w8, #0x1
1004d216c:      b.ne    0x1004d25b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x67c>
1004d2170:      ldr x8, [x0]
1004d2174:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004d2178:      cmp x8, x9
1004d217c:      b.hs    0x1004d25c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x690>
1004d2180:      ldr x8, [x0, #0x20]
1004d2184:      cmp x8, #0x1, lsl #12       ; =0x1000
1004d2188:      b.hi    0x1004d25d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x69c>
1004d218c:      mov x0, x19
1004d2190:      ldp x29, x30, [sp, #0x80]
1004d2194:      ldp x20, x19, [sp, #0x70]
1004d2198:      ldp x22, x21, [sp, #0x60]
1004d219c:      ldp x24, x23, [sp, #0x50]
1004d21a0:      ldp x26, x25, [sp, #0x40]
1004d21a4:      ldp x28, x27, [sp, #0x30]
1004d21a8:      ldp d9, d8, [sp, #0x20]
1004d21ac:      add sp, sp, #0x90
1004d21b0:      ret
1004d21b4:      cmp x10, x22
1004d21b8:      b.eq    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d21bc:      cmp x8, #0x7
1004d21c0:      b.hi    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d21c4:      cmp w12, #0x22
1004d21c8:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d21cc:      sub x19, x8, #0x2
1004d21d0:      cmp x28, x22
1004d21d4:      b.ne    0x1004d2298 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x364>
1004d21d8:      add x8, x0, x26
1004d21dc:      add x20, x8, #0x15
1004d21e0:      add x8, sp, #0x8
1004d21e4:      str x0, [sp]
1004d21e8:      mov x0, x20
1004d21ec:      mov x27, x1
1004d21f0:      mov x1, x19
1004d21f4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004d21f8:      ldp x0, x8, [sp]
1004d21fc:      mov x1, x27
1004d2200:      cbnz    x8, 0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2204:      cmp x28, x22
1004d2208:      b.ne    0x1004d2324 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3f0>
1004d220c:      mov x12, #0x0               ; =0
1004d2210:      b   0x1004d25a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d2214:      cmp w9, #0x74
1004d2218:      b.eq    0x1004d225c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x328>
1004d221c:      cmp w9, #0x6e
1004d2220:      b.ne    0x1004d20c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x190>
1004d2224:      add x8, x0, x26
1004d2228:      ldrb    w9, [x8, #0x15]
1004d222c:      cmp w9, #0x75
1004d2230:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2234:      ldrb    w8, [x8, #0x16]
1004d2238:      cmp w8, #0x6c
1004d223c:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2240:      add x8, x0, x26
1004d2244:      ldrb    w8, [x8, #0x17]
1004d2248:      cmp w8, #0x6c
1004d224c:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2250:      mov x19, #0x2               ; =2
1004d2254:      movk    x19, #0x7ffc, lsl #48
1004d2258:      b   0x1004d20fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d225c:      add x8, x0, x26
1004d2260:      ldrb    w9, [x8, #0x15]
1004d2264:      cmp w9, #0x72
1004d2268:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d226c:      ldrb    w8, [x8, #0x16]
1004d2270:      cmp w8, #0x75
1004d2274:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2278:      add x8, x0, x26
1004d227c:      ldrb    w8, [x8, #0x17]
1004d2280:      cmp w8, #0x65
1004d2284:      b.ne    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2288:      mov x8, #0x2                ; =2
1004d228c:      movk    x8, #0x7ffc, lsl #48
1004d2290:      add x19, x8, #0x2
1004d2294:      b   0x1004d20fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d2298:      mov x8, #0x0                ; =0
1004d229c:      add x9, x0, x8
1004d22a0:      add x9, x9, x26
1004d22a4:      ldrb    w9, [x9, #0x15]
1004d22a8:      cmp w9, #0x20
1004d22ac:      b.lo    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d22b0:      cmp w9, #0x22
1004d22b4:      b.eq    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d22b8:      cmp w9, #0x5c
1004d22bc:      b.eq    0x1004d1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d22c0:      add x8, x8, #0x1
1004d22c4:      cmp x19, x8
1004d22c8:      b.ne    0x1004d229c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x368>
1004d22cc:      b   0x1004d21d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2a4>
1004d22d0:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d22d4:      add x0, x0, #0xda8
1004d22d8:      ldr x8, [x0]
1004d22dc:      blr x8
1004d22e0:      ldr x8, [x0]
1004d22e4:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d22e8:      add x0, x0, #0x808
1004d22ec:      ldr x9, [x0]
1004d22f0:      blr x9
1004d22f4:      ldr x9, [x0]
1004d22f8:      cmp x9, x8
1004d22fc:      b.ls    0x1004d231c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3e8>
1004d2300:      str x8, [x0]
1004d2304:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2308:      add x0, x0, #0x778
1004d230c:      ldr x8, [x0]
1004d2310:      blr x8
1004d2314:      mov w8, #0x1                ; =1
1004d2318:      strb    w8, [x0]
1004d231c:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004d2320:      b   0x1004d2154 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x220>
1004d2324:      cmp x19, #0x4
1004d2328:      b.hs    0x1004d2338 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x404>
1004d232c:      mov x12, #0x0               ; =0
1004d2330:      mov x8, #0x0                ; =0
1004d2334:      b   0x1004d2580 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x64c>
1004d2338:      adrp    x10, 0x100c29000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x3ae8>
1004d233c:      adrp    x9, 0x100bd0000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x3369a>
1004d2340:      cmp x19, #0x10
1004d2344:      b.hs    0x1004d2354 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x420>
1004d2348:      mov x8, #0x0                ; =0
1004d234c:      mov x12, #0x0               ; =0
1004d2350:      b   0x1004d24d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x5a4>
1004d2354:      mov x12, #0x0               ; =0
1004d2358:      and x11, x19, #0xc
1004d235c:      adrp    x8, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d2360:      ldr q0, [x8, #0x780]
1004d2364:      adrp    x8, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d2368:      ldr q1, [x8, #0x790]
1004d236c:      and x8, x19, #0xfffffffffffffff0
1004d2370:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d2374:      ldr q2, [x13, #0x7a0]
1004d2378:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d237c:      ldr q3, [x13, #0x7b0]
1004d2380:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d2384:      ldr q4, [x13, #0x7c0]
1004d2388:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x4ae8>
1004d238c:      ldr q5, [x13, #0x7d0]
1004d2390:      mov w13, #0x10              ; =16
1004d2394:      dup.2d  v7, x13
1004d2398:      and x13, x25, #0xfffffffffffffff0
1004d239c:      add x13, x0, x13
1004d23a0:      sub x20, x13, x24
1004d23a4:      movi.2d v6, #0000000000000000
1004d23a8:      ldr q16, [x10, #0xa00]
1004d23ac:      movi.2d v17, #0000000000000000
1004d23b0:      movi.2d v19, #0000000000000000
1004d23b4:      ldr q22, [x9, #0x8e0]
1004d23b8:      movi.2d v18, #0000000000000000
1004d23bc:      movi.2d v23, #0000000000000000
1004d23c0:      movi.2d v20, #0000000000000000
1004d23c4:      movi.2d v24, #0000000000000000
1004d23c8:      movi.2d v21, #0000000000000000
1004d23cc:      add x13, x0, x12
1004d23d0:      add x13, x13, x26
1004d23d4:      ldur    q25, [x13, #0x15]
1004d23d8:      ushll2.8h   v26, v25, #0x0
1004d23dc:      ushll2.4s   v27, v26, #0x0
1004d23e0:      ushll.2d    v28, v27, #0x0
1004d23e4:      ushll.4s    v26, v26, #0x0
1004d23e8:      ushll2.2d   v29, v26, #0x0
1004d23ec:      ushll.8h    v25, v25, #0x0
1004d23f0:      ushll2.4s   v30, v25, #0x0
1004d23f4:      ushll2.2d   v31, v30, #0x0
1004d23f8:      ushll2.2d   v27, v27, #0x0
1004d23fc:      ushll.2d    v26, v26, #0x0
1004d2400:      ushll.2d    v30, v30, #0x0
1004d2404:      ushll.4s    v25, v25, #0x0
1004d2408:      ushll2.2d   v8, v25, #0x0
1004d240c:      ushll.2d    v25, v25, #0x0
1004d2410:      shl.2d  v9, v22, #0x3
1004d2414:      ushl.2d v25, v25, v9
1004d2418:      shl.2d  v9, v16, #0x3
1004d241c:      ushl.2d v8, v8, v9
1004d2420:      shl.2d  v9, v5, #0x3
1004d2424:      ushl.2d v30, v30, v9
1004d2428:      shl.2d  v9, v3, #0x3
1004d242c:      ushl.2d v26, v26, v9
1004d2430:      shl.2d  v9, v0, #0x3
1004d2434:      ushl.2d v27, v27, v9
1004d2438:      shl.2d  v9, v4, #0x3
1004d243c:      ushl.2d v31, v31, v9
1004d2440:      shl.2d  v9, v2, #0x3
1004d2444:      ushl.2d v29, v29, v9
1004d2448:      shl.2d  v9, v1, #0x3
1004d244c:      ushl.2d v28, v28, v9
1004d2450:      orr.16b v24, v28, v24
1004d2454:      orr.16b v20, v29, v20
1004d2458:      orr.16b v18, v31, v18
1004d245c:      orr.16b v21, v27, v21
1004d2460:      orr.16b v23, v26, v23
1004d2464:      orr.16b v19, v30, v19
1004d2468:      orr.16b v6, v8, v6
1004d246c:      orr.16b v17, v25, v17
1004d2470:      add x12, x12, #0x10
1004d2474:      add.2d  v5, v5, v7
1004d2478:      add.2d  v16, v16, v7
1004d247c:      add.2d  v22, v22, v7
1004d2480:      add.2d  v4, v4, v7
1004d2484:      add.2d  v3, v3, v7
1004d2488:      add.2d  v2, v2, v7
1004d248c:      add.2d  v1, v1, v7
1004d2490:      add.2d  v0, v0, v7
1004d2494:      cmp x8, x12
1004d2498:      b.ne    0x1004d23cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x498>
1004d249c:      orr.16b v0, v17, v23
1004d24a0:      orr.16b v1, v19, v24
1004d24a4:      orr.16b v0, v0, v1
1004d24a8:      orr.16b v1, v6, v20
1004d24ac:      orr.16b v2, v18, v21
1004d24b0:      orr.16b v1, v1, v2
1004d24b4:      orr.16b v0, v0, v1
1004d24b8:      mov d1, v0[1]
1004d24bc:      orr.8b  v0, v0, v1
1004d24c0:      fmov    x12, d0
1004d24c4:      cmp x19, x8
1004d24c8:      b.eq    0x1004d25a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d24cc:      mov x1, x27
1004d24d0:      ldr x0, [sp]
1004d24d4:      cbz x11, 0x1004d2580 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x64c>
1004d24d8:      mov x11, x8
1004d24dc:      and x8, x19, #0xfffffffffffffffc
1004d24e0:      and x13, x25, #0xfffffffffffffffc
1004d24e4:      add x13, x0, x13
1004d24e8:      sub x20, x13, x24
1004d24ec:      fmov    d0, x12
1004d24f0:      movi.2d v1, #0000000000000000
1004d24f4:      dup.2d  v3, x11
1004d24f8:      ldr q2, [x10, #0xa00]
1004d24fc:      orr.16b v2, v3, v2
1004d2500:      ldr q4, [x9, #0x8e0]
1004d2504:      orr.16b v3, v3, v4
1004d2508:      sub x9, x11, x8
1004d250c:      add x10, x0, x11
1004d2510:      add x10, x10, x26
1004d2514:      add x10, x10, #0x15
1004d2518:      movi.2d v4, #0x000000000000ff
1004d251c:      mov w11, #0x4               ; =4
1004d2520:      dup.2d  v5, x11
1004d2524:      ldr s6, [x10], #0x4
1004d2528:      ushll.8h    v6, v6, #0x0
1004d252c:      ushll.4s    v6, v6, #0x0
1004d2530:      ushll2.2d   v7, v6, #0x0
1004d2534:      and.16b v7, v7, v4
1004d2538:      ushll.2d    v6, v6, #0x0
1004d253c:      and.16b v6, v6, v4
1004d2540:      shl.2d  v16, v2, #0x3
1004d2544:      shl.2d  v17, v3, #0x3
1004d2548:      ushl.2d v6, v6, v17
1004d254c:      ushl.2d v7, v7, v16
1004d2550:      orr.16b v1, v7, v1
1004d2554:      orr.16b v0, v6, v0
1004d2558:      add.2d  v2, v2, v5
1004d255c:      add.2d  v3, v3, v5
1004d2560:      adds    x9, x9, #0x4
1004d2564:      b.ne    0x1004d2524 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x5f0>
1004d2568:      orr.16b v0, v0, v1
1004d256c:      mov d1, v0[1]
1004d2570:      orr.8b  v0, v0, v1
1004d2574:      fmov    x12, d0
1004d2578:      cmp x19, x8
1004d257c:      b.eq    0x1004d25a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d2580:      add x9, x23, x1
1004d2584:      lsl x8, x8, #3
1004d2588:      ldrb    w10, [x20], #0x1
1004d258c:      lsl x10, x10, x8
1004d2590:      orr x12, x10, x12
1004d2594:      add x8, x8, #0x8
1004d2598:      cmp x9, x20
1004d259c:      b.ne    0x1004d2588 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x654>
1004d25a0:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004d25a4:      orr x8, x21, x8
1004d25a8:      orr x19, x8, x12
1004d25ac:      b   0x1004d20fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d25b0:      bl  0x100abcbb0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004d25b4:      cbnz    x0, 0x1004d2170 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x23c>
1004d25b8:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004d25bc:      add x0, x0, #0x850
1004d25c0:      bl  0x100aef6dc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004d25c4:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004d25c8:      add x0, x0, #0x3c8
1004d25cc:      bl  0x100ab12dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004d25d0:      bl  0x100ade6c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004d25d4:      b   0x1004d218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x258>
