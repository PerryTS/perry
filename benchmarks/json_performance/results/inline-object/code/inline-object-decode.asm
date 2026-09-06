/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/inline-object-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100821084 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>:
100821084:      stp d9, d8, [sp, #-0x70]!
100821088:      stp x28, x27, [sp, #0x10]
10082108c:      stp x26, x25, [sp, #0x20]
100821090:      stp x24, x23, [sp, #0x30]
100821094:      stp x22, x21, [sp, #0x40]
100821098:      stp x20, x19, [sp, #0x50]
10082109c:      stp x29, x30, [sp, #0x60]
1008210a0:      add x29, sp, #0x60
1008210a4:      sub sp, sp, #0x220
1008210a8:      mov x19, x2
1008210ac:      mov x8, #0x0                ; =0
1008210b0:      add x9, sp, #0x108
1008210b4:      add x22, x9, #0x10
1008210b8:      add x25, x9, #0x20
1008210bc:      add x26, x9, #0x30
1008210c0:      add x27, x9, #0x40
1008210c4:      add x28, x9, #0x50
1008210c8:      add x23, x9, #0x60
1008210cc:      add x20, x9, #0x70
1008210d0:      mov x9, #0x2600             ; =9728
1008210d4:      movk    x9, #0x1, lsl #32
1008210d8:      ldrb    w10, [x1, x8]
1008210dc:      cmp w10, #0x20
1008210e0:      b.hi    0x1008210fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x78>
1008210e4:      lsr x11, x9, x10
1008210e8:      tbz w11, #0x0, 0x1008210fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x78>
1008210ec:      add x8, x8, #0x1
1008210f0:      cmp x19, x8
1008210f4:      b.ne    0x1008210d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x54>
1008210f8:      b   0x100821934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8b0>
1008210fc:      cmp w10, #0x7b
100821100:      b.ne    0x100821934 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8b0>
100821104:      stp x0, x1, [sp, #0xf0]
100821108:      add x21, x8, #0x1
10082110c:      str x21, [sp, #0x100]
100821110:      adrp    x8, 0x100db8000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x27a0>
100821114:      add x8, x8, #0xfa0
100821118:      add x0, sp, #0x108
10082111c:      mov x1, x8
100821120:      mov w2, #0x80               ; =128
100821124:      bl  0x100cdb750 <_writev+0x100cdb750>
100821128:      ldr x0, [sp, #0xf8]
10082112c:      mov x12, #0x0               ; =0
100821130:      add x8, x0, #0x1
100821134:      str x8, [sp, #0xe0]
100821138:      mov x24, #0x2600            ; =9728
10082113c:      movk    x24, #0x1, lsl #32
100821140:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
100821144:      ldr q0, [x8, #0x7b0]
100821148:      str q0, [sp, #0xd0]
10082114c:      adrp    x8, 0x100db3000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x33e9a>
100821150:      ldr q0, [x8, #0xe0]
100821154:      str q0, [sp, #0xc0]
100821158:      mov w8, #0x4                ; =4
10082115c:      dup.2d  v16, x8
100821160:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
100821164:      ldr q0, [x8, #0x750]
100821168:      str q0, [sp, #0xa0]
10082116c:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
100821170:      ldr q0, [x8, #0x760]
100821174:      str q0, [sp, #0x90]
100821178:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
10082117c:      ldr q0, [x8, #0x770]
100821180:      str q0, [sp, #0x80]
100821184:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
100821188:      ldr q0, [x8, #0x780]
10082118c:      str q0, [sp, #0x70]
100821190:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
100821194:      ldr q0, [x8, #0x790]
100821198:      str q0, [sp, #0x60]
10082119c:      adrp    x8, 0x100dfb000 <_anon.a3a1760b3424734627bc02f3f6380623.3593+0x2d>
1008211a0:      ldr q1, [x8, #0x7a0]
1008211a4:      mov w8, #0x10               ; =16
1008211a8:      dup.2d  v0, x8
1008211ac:      str xzr, [sp, #0x188]
1008211b0:      str q16, [sp, #0xb0]
1008211b4:      stp q0, q1, [sp, #0x40]
1008211b8:      cmp x21, x19
1008211bc:      b.hs    0x1008211e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x160>
1008211c0:      ldrb    w8, [x0, x21]
1008211c4:      cmp w8, #0x20
1008211c8:      b.hi    0x1008211e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x160>
1008211cc:      lsr x8, x24, x8
1008211d0:      tbz w8, #0x0, 0x1008211e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x160>
1008211d4:      add x21, x21, #0x1
1008211d8:      str x21, [sp, #0x100]
1008211dc:      cmp x21, x19
1008211e0:      b.lo    0x1008211c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x13c>
1008211e4:      str x12, [sp, #0xe8]
1008211e8:      add x2, sp, #0x100
1008211ec:      mov x1, x19
1008211f0:      bl  0x100820dd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object13inline_string>
1008211f4:      cmp x0, #0x1
1008211f8:      b.ne    0x10082195c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8d8>
1008211fc:      ldp x0, x8, [sp, #0xf8]
100821200:      cmp x8, x19
100821204:      ldr x10, [sp, #0xf0]
100821208:      mov x13, #-0x10000000000    ; =-1099511627776
10082120c:      b.hs    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821210:      ldrb    w9, [x0, x8]
100821214:      cmp w9, #0x3a
100821218:      b.hi    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
10082121c:      lsr x11, x24, x9
100821220:      tbz w11, #0x0, 0x100821234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x1b0>
100821224:      add x8, x8, #0x1
100821228:      cmp x19, x8
10082122c:      b.ne    0x100821210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x18c>
100821230:      b   0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821234:      cmp x9, #0x3a
100821238:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
10082123c:      stp x25, x22, [sp, #0x30]
100821240:      add x21, x8, #0x1
100821244:      cmp x21, x19
100821248:      stp x27, x26, [sp, #0x20]
10082124c:      str x28, [sp, #0x18]
100821250:      b.hs    0x1008212a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x21c>
100821254:      ldrb    w8, [x0, x21]
100821258:      cmp w8, #0x20
10082125c:      b.hi    0x100821280 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x1fc>
100821260:      lsr x9, x24, x8
100821264:      tbz w9, #0x0, 0x100821280 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x1fc>
100821268:      add x21, x21, #0x1
10082126c:      cmp x19, x21
100821270:      b.ne    0x100821254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x1d0>
100821274:      mov x21, x19
100821278:      mov x22, x19
10082127c:      b   0x1008212e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x264>
100821280:      str x21, [sp, #0x100]
100821284:      cmp w8, #0x22
100821288:      b.ne    0x1008212a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x21c>
10082128c:      add x2, sp, #0x100
100821290:      mov x21, x1
100821294:      mov x1, x19
100821298:      bl  0x100820dd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object13inline_string>
10082129c:      b   0x100821438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3b4>
1008212a0:      mov x22, x21
1008212a4:      cmp x21, x19
1008212a8:      b.hs    0x1008212e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x260>
1008212ac:      ldrb    w8, [x0, x22]
1008212b0:      cmp w8, #0x2c
1008212b4:      b.hi    0x1008212c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x244>
1008212b8:      mov x9, #0x2600             ; =9728
1008212bc:      movk    x9, #0x1001, lsl #32
1008212c0:      lsr x9, x9, x8
1008212c4:      tbnz    w9, #0x0, 0x1008212e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x264>
1008212c8:      cmp w8, #0x7d
1008212cc:      b.eq    0x1008212e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x264>
1008212d0:      add x22, x22, #0x1
1008212d4:      cmp x19, x22
1008212d8:      b.ne    0x1008212ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x228>
1008212dc:      mov x22, x19
1008212e0:      b   0x1008212e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x264>
1008212e4:      mov x22, x21
1008212e8:      str x22, [sp, #0x100]
1008212ec:      cmp x22, x21
1008212f0:      b.lo    0x100821ab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0xa30>
1008212f4:      cmp x22, x19
1008212f8:      b.hi    0x100821ab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0xa30>
1008212fc:      subs    x8, x22, x21
100821300:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821304:      ldrb    w9, [x0, x21]
100821308:      orr w11, w9, #0x20
10082130c:      cmp w11, #0x7b
100821310:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821314:      sub x27, x8, #0x2
100821318:      sub x11, x22, #0x1
10082131c:      lsl x25, x27, #40
100821320:      cmp w9, #0x20
100821324:      b.hi    0x100821358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x2d4>
100821328:      mov w8, w9
10082132c:      lsr x8, x24, x8
100821330:      tbz w8, #0x0, 0x100821358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x2d4>
100821334:      cmp x11, x21
100821338:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
10082133c:      add x8, x0, x21
100821340:      ldrb    w9, [x8, #0x1]
100821344:      add x21, x21, #0x1
100821348:      sub x27, x27, #0x1
10082134c:      add x25, x25, x13
100821350:      cmp w9, #0x20
100821354:      b.ls    0x100821328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x2a4>
100821358:      sub x26, x0, #0x1
10082135c:      mov x28, x21
100821360:      ldrb    w12, [x26, x22]
100821364:      cmp w12, #0x20
100821368:      b.hi    0x100821390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x30c>
10082136c:      lsr x8, x24, x12
100821370:      tbz w8, #0x0, 0x100821390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x30c>
100821374:      sub x26, x26, #0x1
100821378:      add x28, x28, #0x1
10082137c:      add x25, x25, x13
100821380:      sub x27, x27, #0x1
100821384:      cmp x22, x28
100821388:      b.ne    0x100821360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x2dc>
10082138c:      b   0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821390:      sub x8, x22, x28
100821394:      cmp x8, #0x4
100821398:      b.eq    0x100821404 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x380>
10082139c:      cmp x8, #0x5
1008213a0:      b.ne    0x10082140c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x388>
1008213a4:      cmp w9, #0x22
1008213a8:      b.eq    0x100821588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x504>
1008213ac:      cmp w9, #0x2d
1008213b0:      b.eq    0x100821428 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3a4>
1008213b4:      cmp w9, #0x66
1008213b8:      b.ne    0x10082141c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x398>
1008213bc:      add x8, x0, x21
1008213c0:      ldrb    w9, [x8, #0x1]
1008213c4:      cmp w9, #0x61
1008213c8:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008213cc:      ldrb    w8, [x8, #0x2]
1008213d0:      cmp w8, #0x6c
1008213d4:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008213d8:      add x8, x0, x21
1008213dc:      ldrb    w9, [x8, #0x3]
1008213e0:      cmp w9, #0x73
1008213e4:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008213e8:      ldrb    w8, [x8, #0x4]
1008213ec:      cmp w8, #0x65
1008213f0:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008213f4:      mov x8, #0x2                ; =2
1008213f8:      movk    x8, #0x7ffc, lsl #48
1008213fc:      orr x8, x8, #0x1
100821400:      b   0x10082144c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3c8>
100821404:      cmp w9, #0x6d
100821408:      b.gt    0x100821624 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x5a0>
10082140c:      cmp w9, #0x22
100821410:      b.eq    0x100821588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x504>
100821414:      cmp w9, #0x2d
100821418:      b.eq    0x100821428 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3a4>
10082141c:      sub w9, w9, #0x30
100821420:      cmp w9, #0xa
100821424:      b.hs    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821428:      add x0, x0, x21
10082142c:      mov x21, x1
100821430:      mov x1, x8
100821434:      bl  0x10081a464 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar12parse_number>
100821438:      mov x9, x0
10082143c:      ldp x10, x0, [sp, #0xf0]
100821440:      tbz w9, #0x0, 0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821444:      mov x8, x1
100821448:      mov x1, x21
10082144c:      ldr x22, [sp, #0x38]
100821450:      ldr x12, [sp, #0xe8]
100821454:      cmp x12, #0x9
100821458:      b.hs    0x100821acc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0xa48>
10082145c:      ldp x26, x25, [sp, #0x28]
100821460:      ldp x28, x27, [sp, #0x18]
100821464:      cbz x12, 0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
100821468:      ldr x9, [sp, #0x108]
10082146c:      cmp x9, x1
100821470:      b.ne    0x100821480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3fc>
100821474:      add x9, sp, #0x108
100821478:      str x8, [x9, #0x8]
10082147c:      b   0x1008214a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x41c>
100821480:      cmp x12, #0x1
100821484:      b.ne    0x1008214dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x458>
100821488:      add x9, sp, #0x108
10082148c:      add x9, x9, x12, lsl #4
100821490:      stp x1, x8, [x9]
100821494:      ldr x8, [sp, #0x188]
100821498:      add x12, x8, #0x1
10082149c:      str x12, [sp, #0x188]
1008214a0:      ldr x21, [sp, #0x100]
1008214a4:      cmp x21, x19
1008214a8:      b.hs    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008214ac:      ldrb    w8, [x0, x21]
1008214b0:      cmp w8, #0x2c
1008214b4:      b.hi    0x100821968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8e4>
1008214b8:      lsr x9, x24, x8
1008214bc:      tbz w9, #0x0, 0x1008214d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x44c>
1008214c0:      add x21, x21, #0x1
1008214c4:      cmp x19, x21
1008214c8:      b.ne    0x1008214ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x428>
1008214cc:      b   0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008214d0:      cmp x8, #0x2c
1008214d4:      b.eq    0x1008211d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x150>
1008214d8:      b   0x100821968 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8e4>
1008214dc:      ldr x11, [sp, #0x118]
1008214e0:      mov x9, x22
1008214e4:      cmp x11, x1
1008214e8:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
1008214ec:      cmp x12, #0x2
1008214f0:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
1008214f4:      ldr x11, [sp, #0x128]
1008214f8:      mov x9, x25
1008214fc:      cmp x11, x1
100821500:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
100821504:      cmp x12, #0x3
100821508:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
10082150c:      ldr x11, [sp, #0x138]
100821510:      mov x9, x26
100821514:      cmp x11, x1
100821518:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
10082151c:      cmp x12, #0x4
100821520:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
100821524:      ldr x11, [sp, #0x148]
100821528:      mov x9, x27
10082152c:      cmp x11, x1
100821530:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
100821534:      cmp x12, #0x5
100821538:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
10082153c:      ldr x11, [sp, #0x158]
100821540:      mov x9, x28
100821544:      cmp x11, x1
100821548:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
10082154c:      cmp x12, #0x6
100821550:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
100821554:      ldr x11, [sp, #0x168]
100821558:      mov x9, x23
10082155c:      cmp x11, x1
100821560:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
100821564:      cmp x12, #0x7
100821568:      b.eq    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
10082156c:      ldr x11, [sp, #0x178]
100821570:      mov x9, x20
100821574:      cmp x11, x1
100821578:      b.eq    0x100821478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3f4>
10082157c:      cmp x12, #0x8
100821580:      b.ne    0x100821488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x404>
100821584:      b   0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821588:      cmp x11, x28
10082158c:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821590:      cmp x8, #0x7
100821594:      b.hi    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821598:      cmp w12, #0x22
10082159c:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008215a0:      sub x12, x22, #0x2
1008215a4:      add x9, x0, #0x1
1008215a8:      mov x11, x27
1008215ac:      cmp x12, x28
1008215b0:      b.ne    0x1008215f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x574>
1008215b4:      sub x9, x8, #0x2
1008215b8:      add x8, x0, x21
1008215bc:      add x0, x8, #0x1
1008215c0:      sub x8, x29, #0xf0
1008215c4:      stp x0, x9, [sp]
1008215c8:      str x1, [sp, #0x10]
1008215cc:      mov x1, x9
1008215d0:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008215d4:      sub x9, x22, #0x2
1008215d8:      ldr x1, [sp, #0x10]
1008215dc:      ldp x10, x0, [sp, #0xf0]
1008215e0:      ldur    x8, [x29, #-0xf0]
1008215e4:      cbnz    x8, 0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008215e8:      cmp x9, x28
1008215ec:      b.ne    0x10082166c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x5e8>
1008215f0:      mov x12, #0x0               ; =0
1008215f4:      b   0x100821924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8a0>
1008215f8:      ldrb    w12, [x9, x21]
1008215fc:      cmp w12, #0x20
100821600:      b.lo    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821604:      cmp w12, #0x22
100821608:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
10082160c:      cmp w12, #0x5c
100821610:      b.eq    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821614:      add x9, x9, #0x1
100821618:      subs    x11, x11, #0x1
10082161c:      b.ne    0x1008215f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x574>
100821620:      b   0x1008215b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x530>
100821624:      cmp w9, #0x74
100821628:      b.eq    0x100821690 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x60c>
10082162c:      cmp w9, #0x6e
100821630:      b.ne    0x10082141c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x398>
100821634:      add x8, x0, x21
100821638:      ldrb    w9, [x8, #0x1]
10082163c:      cmp w9, #0x75
100821640:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821644:      ldrb    w8, [x8, #0x2]
100821648:      cmp w8, #0x6c
10082164c:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821650:      add x8, x0, x21
100821654:      ldrb    w8, [x8, #0x3]
100821658:      cmp w8, #0x6c
10082165c:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821660:      mov x8, #0x2                ; =2
100821664:      movk    x8, #0x7ffc, lsl #48
100821668:      b   0x10082144c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3c8>
10082166c:      ldr x8, [sp, #0x8]
100821670:      cmp x8, #0x4
100821674:      b.hs    0x1008216cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x648>
100821678:      mov x12, #0x0               ; =0
10082167c:      mov x8, #0x0                ; =0
100821680:      ldp x10, x0, [sp, #0xf0]
100821684:      ldr x1, [sp, #0x10]
100821688:      ldr x11, [sp]
10082168c:      b   0x100821904 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x880>
100821690:      add x8, x0, x21
100821694:      ldrb    w9, [x8, #0x1]
100821698:      cmp w9, #0x72
10082169c:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008216a0:      ldrb    w8, [x8, #0x2]
1008216a4:      cmp w8, #0x75
1008216a8:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008216ac:      add x8, x0, x21
1008216b0:      ldrb    w8, [x8, #0x3]
1008216b4:      cmp w8, #0x65
1008216b8:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
1008216bc:      mov x8, #0x2                ; =2
1008216c0:      movk    x8, #0x7ffc, lsl #48
1008216c4:      add x8, x8, #0x2
1008216c8:      b   0x10082144c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3c8>
1008216cc:      ldr x8, [sp, #0x8]
1008216d0:      cmp x8, #0x10
1008216d4:      b.hs    0x1008216ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x668>
1008216d8:      mov x8, #0x0                ; =0
1008216dc:      mov x12, #0x0               ; =0
1008216e0:      ldr q16, [sp, #0xb0]
1008216e4:      movi.2d v17, #0x000000000000ff
1008216e8:      b   0x100821860 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x7dc>
1008216ec:      ldr x8, [sp, #0x8]
1008216f0:      and x9, x8, #0xc
1008216f4:      and x8, x8, #0xfffffffffffffff0
1008216f8:      ldr x10, [sp, #0xe0]
1008216fc:      add x10, x10, x8
100821700:      add x11, x10, x21
100821704:      and x10, x27, #0xfffffffffffffff0
100821708:      movi.2d v0, #0000000000000000
10082170c:      ldr x12, [sp, #0xf8]
100821710:      add x12, x12, #0x1
100821714:      movi.2d v2, #0000000000000000
100821718:      movi.2d v3, #0000000000000000
10082171c:      movi.2d v1, #0000000000000000
100821720:      movi.2d v5, #0000000000000000
100821724:      movi.2d v4, #0000000000000000
100821728:      movi.2d v6, #0000000000000000
10082172c:      ldp q16, q17, [sp, #0xc0]
100821730:      ldp q18, q19, [sp, #0x50]
100821734:      movi.2d v7, #0000000000000000
100821738:      ldp q20, q21, [sp, #0x70]
10082173c:      ldp q22, q23, [sp, #0x90]
100821740:      ldr q9, [sp, #0x40]
100821744:      ldr q24, [x12, x21]
100821748:      ushll.8h    v25, v24, #0x0
10082174c:      ushll2.4s   v26, v25, #0x0
100821750:      ushll2.2d   v27, v26, #0x0
100821754:      ushll2.8h   v24, v24, #0x0
100821758:      ushll.4s    v28, v24, #0x0
10082175c:      ushll2.2d   v29, v28, #0x0
100821760:      ushll.2d    v28, v28, #0x0
100821764:      ushll.2d    v26, v26, #0x0
100821768:      ushll.4s    v25, v25, #0x0
10082176c:      ushll2.2d   v30, v25, #0x0
100821770:      ushll2.4s   v24, v24, #0x0
100821774:      ushll.2d    v31, v24, #0x0
100821778:      ushll.2d    v25, v25, #0x0
10082177c:      ushll2.2d   v24, v24, #0x0
100821780:      shl.2d  v8, v23, #0x3
100821784:      ushl.2d v24, v24, v8
100821788:      shl.2d  v8, v16, #0x3
10082178c:      ushl.2d v25, v25, v8
100821790:      shl.2d  v8, v22, #0x3
100821794:      ushl.2d v31, v31, v8
100821798:      shl.2d  v8, v17, #0x3
10082179c:      ushl.2d v30, v30, v8
1008217a0:      shl.2d  v8, v18, #0x3
1008217a4:      ushl.2d v26, v26, v8
1008217a8:      shl.2d  v8, v20, #0x3
1008217ac:      ushl.2d v28, v28, v8
1008217b0:      shl.2d  v8, v21, #0x3
1008217b4:      ushl.2d v29, v29, v8
1008217b8:      shl.2d  v8, v19, #0x3
1008217bc:      ushl.2d v27, v27, v8
1008217c0:      orr.16b v1, v27, v1
1008217c4:      orr.16b v4, v29, v4
1008217c8:      orr.16b v5, v28, v5
1008217cc:      orr.16b v3, v26, v3
1008217d0:      orr.16b v0, v30, v0
1008217d4:      orr.16b v6, v31, v6
1008217d8:      orr.16b v2, v25, v2
1008217dc:      orr.16b v7, v24, v7
1008217e0:      add.2d  v18, v18, v9
1008217e4:      add.2d  v17, v17, v9
1008217e8:      add.2d  v16, v16, v9
1008217ec:      add.2d  v19, v19, v9
1008217f0:      add.2d  v20, v20, v9
1008217f4:      add.2d  v21, v21, v9
1008217f8:      add.2d  v22, v22, v9
1008217fc:      add x12, x12, #0x10
100821800:      add.2d  v23, v23, v9
100821804:      subs    x10, x10, #0x10
100821808:      b.ne    0x100821744 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x6c0>
10082180c:      orr.16b v2, v2, v5
100821810:      orr.16b v3, v3, v6
100821814:      orr.16b v2, v2, v3
100821818:      orr.16b v0, v0, v4
10082181c:      orr.16b v1, v1, v7
100821820:      orr.16b v0, v0, v1
100821824:      orr.16b v0, v2, v0
100821828:      mov d1, v0[1]
10082182c:      orr.8b  v0, v0, v1
100821830:      fmov    x12, d0
100821834:      ldr x10, [sp, #0x8]
100821838:      cmp x10, x8
10082183c:      b.ne    0x10082184c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x7c8>
100821840:      ldp x10, x0, [sp, #0xf0]
100821844:      ldr x1, [sp, #0x10]
100821848:      b   0x100821924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8a0>
10082184c:      ldp x10, x0, [sp, #0xf0]
100821850:      ldr x1, [sp, #0x10]
100821854:      ldr q16, [sp, #0xb0]
100821858:      movi.2d v17, #0x000000000000ff
10082185c:      cbz x9, 0x100821904 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x880>
100821860:      dup.2d  v3, x8
100821864:      ldr x13, [sp, #0xe0]
100821868:      add x10, x13, x8
10082186c:      and x9, x27, #0xfffffffffffffffc
100821870:      sub x11, x8, x9
100821874:      ldr x8, [sp, #0x8]
100821878:      and x8, x8, #0xfffffffffffffffc
10082187c:      add x9, x13, x8
100821880:      add x9, x9, x21
100821884:      fmov    d0, x12
100821888:      movi.2d v1, #0000000000000000
10082188c:      ldp q4, q2, [sp, #0xc0]
100821890:      orr.16b v2, v3, v2
100821894:      orr.16b v3, v3, v4
100821898:      ldr s4, [x10, x21]
10082189c:      ushll.8h    v4, v4, #0x0
1008218a0:      ushll.4s    v4, v4, #0x0
1008218a4:      ushll2.2d   v5, v4, #0x0
1008218a8:      and.16b v5, v5, v17
1008218ac:      ushll.2d    v4, v4, #0x0
1008218b0:      and.16b v4, v4, v17
1008218b4:      shl.2d  v6, v2, #0x3
1008218b8:      shl.2d  v7, v3, #0x3
1008218bc:      ushl.2d v4, v4, v7
1008218c0:      ushl.2d v5, v5, v6
1008218c4:      orr.16b v1, v5, v1
1008218c8:      orr.16b v0, v4, v0
1008218cc:      add.2d  v2, v2, v16
1008218d0:      add.2d  v3, v3, v16
1008218d4:      add x10, x10, #0x4
1008218d8:      adds    x11, x11, #0x4
1008218dc:      b.ne    0x100821898 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x814>
1008218e0:      orr.16b v0, v0, v1
1008218e4:      mov d1, v0[1]
1008218e8:      orr.8b  v0, v0, v1
1008218ec:      fmov    x12, d0
1008218f0:      ldp x10, x1, [sp, #0x8]
1008218f4:      cmp x10, x8
1008218f8:      ldp x10, x0, [sp, #0xf0]
1008218fc:      mov x11, x9
100821900:      b.eq    0x100821924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8a0>
100821904:      add x9, x26, x22
100821908:      lsl x8, x8, #3
10082190c:      ldrb    w13, [x11], #0x1
100821910:      lsl x13, x13, x8
100821914:      orr x12, x13, x12
100821918:      add x8, x8, #0x8
10082191c:      cmp x11, x9
100821920:      b.ne    0x10082190c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x888>
100821924:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
100821928:      orr x8, x25, x8
10082192c:      orr x8, x8, x12
100821930:      b   0x10082144c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x3c8>
100821934:      str xzr, [x0]
100821938:      add sp, sp, #0x220
10082193c:      ldp x29, x30, [sp, #0x60]
100821940:      ldp x20, x19, [sp, #0x50]
100821944:      ldp x22, x21, [sp, #0x40]
100821948:      ldp x24, x23, [sp, #0x30]
10082194c:      ldp x26, x25, [sp, #0x20]
100821950:      ldp x28, x27, [sp, #0x10]
100821954:      ldp d9, d8, [sp], #0x70
100821958:      ret
10082195c:      ldr x8, [sp, #0xf0]
100821960:      str xzr, [x8]
100821964:      b   0x100821938 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8b4>
100821968:      cmp w8, #0x7d
10082196c:      b.ne    0x100821a00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x97c>
100821970:      add x8, x21, #0x1
100821974:      cmp x8, x19
100821978:      b.hs    0x100821a08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x984>
10082197c:      mov x9, #0x2600             ; =9728
100821980:      movk    x9, #0x1, lsl #32
100821984:      ldrb    w11, [x0, x8]
100821988:      cmp w11, #0x20
10082198c:      b.hi    0x100821a08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x984>
100821990:      lsr x11, x9, x11
100821994:      tbz w11, #0x0, 0x100821a08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x984>
100821998:      add x8, x8, #0x1
10082199c:      cmp x19, x8
1008219a0:      b.ne    0x100821984 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x900>
1008219a4:      ldr x8, [sp, #0x188]
1008219a8:      stur    x8, [x29, #-0x70]
1008219ac:      add x8, sp, #0x49
1008219b0:      ldur    q0, [x8, #0xff]
1008219b4:      add x8, sp, #0x59
1008219b8:      ldur    q1, [x8, #0xff]
1008219bc:      stp q0, q1, [x29, #-0xb0]
1008219c0:      add x8, sp, #0x69
1008219c4:      ldur    q0, [x8, #0xff]
1008219c8:      add x8, sp, #0x79
1008219cc:      ldur    q1, [x8, #0xff]
1008219d0:      stp q0, q1, [x29, #-0x90]
1008219d4:      add x8, sp, #0x9
1008219d8:      ldur    q0, [x8, #0xff]
1008219dc:      add x8, sp, #0x19
1008219e0:      ldur    q1, [x8, #0xff]
1008219e4:      stp q0, q1, [x29, #-0xf0]
1008219e8:      add x8, sp, #0x29
1008219ec:      ldur    q0, [x8, #0xff]
1008219f0:      add x8, sp, #0x39
1008219f4:      ldur    q1, [x8, #0xff]
1008219f8:      stp q0, q1, [x29, #-0xd0]
1008219fc:      b   0x100821a68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x9e4>
100821a00:      str xzr, [x10]
100821a04:      b   0x100821938 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8b4>
100821a08:      ldr x9, [sp, #0x188]
100821a0c:      stur    x9, [x29, #-0x70]
100821a10:      add x9, sp, #0x49
100821a14:      ldur    q0, [x9, #0xff]
100821a18:      add x9, sp, #0x59
100821a1c:      ldur    q1, [x9, #0xff]
100821a20:      stp q0, q1, [x29, #-0xb0]
100821a24:      add x9, sp, #0x69
100821a28:      ldur    q0, [x9, #0xff]
100821a2c:      add x9, sp, #0x79
100821a30:      ldur    q1, [x9, #0xff]
100821a34:      stp q0, q1, [x29, #-0x90]
100821a38:      add x9, sp, #0x9
100821a3c:      ldur    q0, [x9, #0xff]
100821a40:      add x9, sp, #0x19
100821a44:      ldur    q1, [x9, #0xff]
100821a48:      stp q0, q1, [x29, #-0xf0]
100821a4c:      add x9, sp, #0x29
100821a50:      ldur    q0, [x9, #0xff]
100821a54:      add x9, sp, #0x39
100821a58:      ldur    q1, [x9, #0xff]
100821a5c:      stp q0, q1, [x29, #-0xd0]
100821a60:      cmp x8, x19
100821a64:      b.ne    0x100821aa8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0xa24>
100821a68:      ldp q0, q1, [x29, #-0xb0]
100821a6c:      stur    q0, [x10, #0x48]
100821a70:      stur    q1, [x10, #0x58]
100821a74:      ldp q0, q1, [x29, #-0x90]
100821a78:      stur    q0, [x10, #0x68]
100821a7c:      stur    q1, [x10, #0x78]
100821a80:      ldp q0, q1, [x29, #-0xf0]
100821a84:      stur    q0, [x10, #0x8]
100821a88:      stur    q1, [x10, #0x18]
100821a8c:      ldp q0, q1, [x29, #-0xd0]
100821a90:      stur    q0, [x10, #0x28]
100821a94:      ldur    x8, [x29, #-0x70]
100821a98:      str x8, [x10, #0x88]
100821a9c:      mov w8, #0x1                ; =1
100821aa0:      stur    q1, [x10, #0x38]
100821aa4:      b   0x100821aac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0xa28>
100821aa8:      mov x8, #0x0                ; =0
100821aac:      str x8, [x10]
100821ab0:      b   0x100821938 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode+0x8b4>
100821ab4:      adrp    x3, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100821ab8:      add x3, x3, #0xf78
100821abc:      mov x0, x21
100821ac0:      mov x1, x22
100821ac4:      mov x2, x19
100821ac8:      bl  0x100c8b54c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
100821acc:      adrp    x3, 0x1010cc000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry17prototype_methods23CLASS_PROTOTYPE_METHODS+0x10>
100821ad0:      add x3, x3, #0xf90
100821ad4:      mov x0, #0x0                ; =0
100821ad8:      mov x1, x12
100821adc:      mov w2, #0x8                ; =8
100821ae0:      bl  0x100c8b54c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
