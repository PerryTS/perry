
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/compact-piece-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004650f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
1004650f8:      sub sp, sp, #0x40
1004650fc:      stp x20, x19, [sp, #0x20]
100465100:      stp x29, x30, [sp, #0x30]
100465104:      add x29, sp, #0x30
100465108:      strb    wzr, [sp, #0xc]
10046510c:      str wzr, [sp, #0x8]
100465110:      and x9, x1, #0xffff000000000000
100465114:      mov x8, #0x7fff000000000000 ; =9223090561878065152
100465118:      cmp x9, x8
10046511c:      b.eq    0x100465198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
100465120:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
100465124:      cmp x9, x8
100465128:      b.ne    0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10046512c:      ubfx    x8, x1, #40, #8
100465130:      cbz x8, 0x100465180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100465134:      strb    w1, [sp, #0x8]
100465138:      cmp x8, #0x1
10046513c:      b.eq    0x100465180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100465140:      lsr x10, x1, #8
100465144:      strb    w10, [sp, #0x9]
100465148:      cmp x8, #0x2
10046514c:      b.eq    0x100465180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100465150:      lsr x10, x1, #16
100465154:      strb    w10, [sp, #0xa]
100465158:      cmp x8, #0x3
10046515c:      b.eq    0x100465180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100465160:      lsr x10, x1, #24
100465164:      strb    w10, [sp, #0xb]
100465168:      cmp x8, #0x4
10046516c:      b.eq    0x100465180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100465170:      lsr x10, x1, #32
100465174:      strb    w10, [sp, #0xc]
100465178:      cmp x8, #0x5
10046517c:      b.ne    0x10046568c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x594>
100465180:      mov x10, x1
100465184:      add x1, sp, #0x8
100465188:      mov w2, w8
10046518c:      cmp w8, #0x10
100465190:      b.hs    0x1004651c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
100465194:      b   0x100465210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
100465198:      ands    x11, x1, #0xffffffffffff
10046519c:      b.eq    0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004651a0:      ldr w8, [x11, #0x4]
1004651a4:      cmn w8, #0x3
1004651a8:      b.hi    0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004651ac:      mov x10, x1
1004651b0:      add x1, x11, #0x14
1004651b4:      mov w2, w8
1004651b8:      cmp w8, #0x10
1004651bc:      b.lo    0x100465210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x118>
1004651c0:      mov w11, #0x10              ; =16
1004651c4:      movi.16b    v0, #0x22
1004651c8:      movi.16b    v1, #0x5c
1004651cc:      movi.16b    v2, #0x20
1004651d0:      mov x12, x1
1004651d4:      movi.16b    v3, #0xed
1004651d8:      ldr q4, [x12], #0x10
1004651dc:      cmeq.16b    v5, v4, v0
1004651e0:      cmeq.16b    v6, v4, v1
1004651e4:      orr.16b v5, v6, v5
1004651e8:      cmhi.16b    v6, v2, v4
1004651ec:      cmeq.16b    v4, v4, v3
1004651f0:      orr.16b v4, v4, v6
1004651f4:      orr.16b v4, v4, v5
1004651f8:      addp.2d d4, v4
1004651fc:      fmov    x13, d4
100465200:      cbnz    x13, 0x10046540c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x314>
100465204:      add x11, x11, #0x10
100465208:      cmp x11, x2
10046520c:      b.ls    0x1004651d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xe0>
100465210:      and x12, x2, #0xfffffff0
100465214:      add x13, x1, x12
100465218:      tbnz    w2, #0x3, 0x10046526c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x174>
10046521c:      tst x2, #0x7
100465220:      b.eq    0x100465578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x480>
100465224:      and x11, x2, #0x8
100465228:      add x13, x13, x11
10046522c:      sub x11, x2, x11
100465230:      sub x12, x11, x12
100465234:      mov w11, #0x0               ; =0
100465238:      ldrb    w14, [x13], #0x1
10046523c:      cmp w14, #0x22
100465240:      b.eq    0x10046557c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
100465244:      cmp w14, #0x5c
100465248:      b.eq    0x10046557c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
10046524c:      mov w11, #0x0               ; =0
100465250:      cmp w14, #0x20
100465254:      b.lo    0x10046557c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
100465258:      cmp w14, #0xed
10046525c:      b.eq    0x10046557c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x484>
100465260:      subs    x12, x12, #0x1
100465264:      b.ne    0x100465234 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
100465268:      b   0x1004653f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
10046526c:      mov x14, #0x101010101010101 ; =72340172838076673
100465270:      movk    x14, #0x100
100465274:      ldr x11, [x13]
100465278:      eor x15, x11, #0x2222222222222222
10046527c:      sub x15, x14, x15
100465280:      orr x16, x15, x11
100465284:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
100465288:      bics    xzr, x15, x16
10046528c:      b.ne    0x1004652ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
100465290:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
100465294:      orr x16, x16, #0x4444444444444444
100465298:      eor x16, x11, x16
10046529c:      sub x14, x14, x16
1004652a0:      orr x14, x14, x11
1004652a4:      bics    xzr, x15, x14
1004652a8:      b.ne    0x1004652ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x1f4>
1004652ac:      mov x14, #0x2020202020202020 ; =2314885530818453536
1004652b0:      movk    x14, #0x201f
1004652b4:      sub x14, x14, x11
1004652b8:      orr x14, x14, x11
1004652bc:      bic x14, x15, x14
1004652c0:      cmp x14, #0x0
1004652c4:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
1004652c8:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
1004652cc:      eor x14, x11, x14
1004652d0:      mov x15, #-0x101010101010102 ; =-72340172838076674
1004652d4:      movk    x15, #0xfeff
1004652d8:      add x14, x14, x15
1004652dc:      and x14, x11, x14
1004652e0:      and x14, x14, #0x8080808080808080
1004652e4:      ccmp    x14, #0x0, #0x0, eq
1004652e8:      b.eq    0x10046521c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x124>
1004652ec:      and w12, w11, #0xff
1004652f0:      cmp w12, #0x22
1004652f4:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004652f8:      cmp w12, #0x5c
1004652fc:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465300:      cmp w12, #0x20
100465304:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465308:      cmp w12, #0xed
10046530c:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465310:      ubfx    w12, w11, #8, #8
100465314:      cmp w12, #0x22
100465318:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
10046531c:      cmp w12, #0x5c
100465320:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465324:      cmp w12, #0xed
100465328:      mov w13, #0x20              ; =32
10046532c:      ccmp    w12, w13, #0x0, ne
100465330:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465334:      ubfx    w12, w11, #16, #8
100465338:      cmp w12, #0x22
10046533c:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465340:      cmp w12, #0x5c
100465344:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465348:      cmp w12, #0xed
10046534c:      ccmp    w12, w13, #0x0, ne
100465350:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465354:      lsr w12, w11, #24
100465358:      cmp w12, #0x22
10046535c:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465360:      cmp w12, #0x5c
100465364:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465368:      cmp w12, #0xed
10046536c:      ccmp    w12, w13, #0x0, ne
100465370:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465374:      ubfx    x12, x11, #32, #8
100465378:      cmp w12, #0x22
10046537c:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465380:      cmp w12, #0x5c
100465384:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465388:      cmp w12, #0xed
10046538c:      ccmp    w12, w13, #0x0, ne
100465390:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
100465394:      ubfx    x12, x11, #40, #8
100465398:      cmp w12, #0x22
10046539c:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653a0:      cmp w12, #0x5c
1004653a4:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653a8:      cmp w12, #0xed
1004653ac:      ccmp    w12, w13, #0x0, ne
1004653b0:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653b4:      ubfx    x12, x11, #48, #8
1004653b8:      cmp w12, #0x22
1004653bc:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653c0:      cmp w12, #0x5c
1004653c4:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653c8:      cmp w12, #0xed
1004653cc:      ccmp    w12, w13, #0x0, ne
1004653d0:      b.lo    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653d4:      lsr x12, x11, #56
1004653d8:      cmp w12, #0x22
1004653dc:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653e0:      cmp w12, #0x5c
1004653e4:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653e8:      lsr x11, x11, #61
1004653ec:      cmp x12, #0xed
1004653f0:      ccmp    x11, #0x0, #0x4, ne
1004653f4:      b.eq    0x100465414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x31c>
1004653f8:      mov w11, #0x1               ; =1
1004653fc:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100465400:      cmp x9, x12
100465404:      b.ne    0x100465424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100465408:      b   0x100465588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
10046540c:      fmov    x11, d4
100465410:      cbz x11, 0x1004653f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x300>
100465414:      mov w11, #0x0               ; =0
100465418:      mov x12, #0x7fff000000000000 ; =9223090561878065152
10046541c:      cmp x9, x12
100465420:      b.eq    0x100465588 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x490>
100465424:      cmp w8, #0x40
100465428:      b.hs    0x1004654ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f4>
10046542c:      ands    x9, x2, #0x38
100465430:      b.eq    0x100465454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x35c>
100465434:      and x10, x2, #0x38
100465438:      neg x10, x10
10046543c:      mov x12, x1
100465440:      ldr x13, [x12], #0x8
100465444:      tst x13, #0x8080808080808080
100465448:      b.ne    0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10046544c:      adds    x10, x10, #0x8
100465450:      b.ne    0x100465440 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x348>
100465454:      mov x3, x8
100465458:      and x10, x2, #0x7
10046545c:      cbz x10, 0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465460:      add x9, x1, x9
100465464:      ldrsb   w12, [x9]
100465468:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
10046546c:      mov x3, x8
100465470:      cmp x10, #0x1
100465474:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465478:      ldrsb   w12, [x9, #0x1]
10046547c:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465480:      mov x3, x8
100465484:      cmp x10, #0x2
100465488:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
10046548c:      ldrsb   w12, [x9, #0x2]
100465490:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465494:      mov x3, x8
100465498:      cmp x10, #0x3
10046549c:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004654a0:      ldrsb   w12, [x9, #0x3]
1004654a4:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004654a8:      mov x3, x8
1004654ac:      cmp x10, #0x4
1004654b0:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004654b4:      ldrsb   w12, [x9, #0x4]
1004654b8:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004654bc:      mov x3, x8
1004654c0:      cmp x10, #0x5
1004654c4:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004654c8:      ldrsb   w12, [x9, #0x5]
1004654cc:      tbnz    w12, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004654d0:      mov x3, x8
1004654d4:      cmp x10, #0x6
1004654d8:      b.eq    0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004654dc:      ldrsb   w9, [x9, #0x6]
1004654e0:      mov x3, x8
1004654e4:      tbz w9, #0x1f, 0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
1004654e8:      b   0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
1004654ec:      and x9, x2, #0xffffffc0
1004654f0:      add x9, x1, x9
1004654f4:      mov x10, x1
1004654f8:      ldp q0, q1, [x10]
1004654fc:      ldp q2, q3, [x10, #0x20]
100465500:      orr.16b v0, v1, v0
100465504:      orr.16b v1, v2, v3
100465508:      orr.16b v0, v0, v1
10046550c:      umaxv.16b   b0, v0
100465510:      fmov    w12, s0
100465514:      tbnz    w12, #0x7, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465518:      add x10, x10, #0x40
10046551c:      cmp x10, x9
100465520:      b.ne    0x1004654f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x400>
100465524:      ands    x10, x2, #0x30
100465528:      b.eq    0x10046554c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x454>
10046552c:      mov x12, x10
100465530:      mov x13, x9
100465534:      ldr q0, [x13], #0x10
100465538:      umaxv.16b   b0, v0
10046553c:      fmov    w14, s0
100465540:      tbnz    w14, #0x7, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465544:      subs    x12, x12, #0x10
100465548:      b.ne    0x100465534 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x43c>
10046554c:      mov x3, x8
100465550:      and x12, x2, #0xf
100465554:      cbz x12, 0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465558:      add x9, x9, x10
10046555c:      ldrsb   w10, [x9]
100465560:      tbnz    w10, #0x1f, 0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465564:      add x9, x9, #0x1
100465568:      subs    x12, x12, #0x1
10046556c:      b.ne    0x10046555c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x464>
100465570:      mov x3, x8
100465574:      b   0x100465590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x498>
100465578:      mov w11, #0x1               ; =1
10046557c:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100465580:      cmp x9, x12
100465584:      b.ne    0x100465424 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x32c>
100465588:      and x9, x10, #0xffffffffffff
10046558c:      ldr w3, [x9]
100465590:      tbz w11, #0x0, 0x1004655cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4d4>
100465594:      mov w9, #0x3                ; =3
100465598:      cmp x2, #0x3
10046559c:      csel    x9, x2, x9, lo
1004655a0:      cbz w8, 0x100465660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x568>
1004655a4:      add x10, x1, x2
1004655a8:      ldurb   w10, [x10, #-0x1]
1004655ac:      cmp w10, #0xbf
1004655b0:      b.ls    0x100465604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x50c>
1004655b4:      mov w8, #0x2                ; =2
1004655b8:      str w8, [x0]
1004655bc:      ldp x29, x30, [sp, #0x30]
1004655c0:      ldp x20, x19, [sp, #0x20]
1004655c4:      add sp, sp, #0x40
1004655c8:      ret
1004655cc:      mov x19, x0
1004655d0:      add x0, sp, #0x10
1004655d4:      bl  0x100435658 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
1004655d8:      ldr w8, [sp, #0x10]
1004655dc:      tbz w8, #0x0, 0x1004655f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x500>
1004655e0:      ldur    x9, [sp, #0x14]
1004655e4:      stur    x9, [x19, #0x4]
1004655e8:      ldr w9, [sp, #0x1c]
1004655ec:      str w9, [x19, #0xc]
1004655f0:      str wzr, [x19]
1004655f4:      b   0x1004655bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
1004655f8:      mov w8, #0x2                ; =2
1004655fc:      str w8, [x19]
100465600:      b   0x1004655bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100465604:      cmp w8, #0x1
100465608:      b.eq    0x100465660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x568>
10046560c:      add x10, x1, x2
100465610:      ldurb   w10, [x10, #-0x2]
100465614:      cmp w10, #0xdf
100465618:      b.ls    0x100465624 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x52c>
10046561c:      mov w10, #0x2               ; =2
100465620:      b   0x100465658 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x560>
100465624:      cmp w8, #0x2
100465628:      b.eq    0x100465660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x568>
10046562c:      add x11, x1, x2
100465630:      mov x10, #-0x3              ; =-3
100465634:      ldrb    w12, [x11, x10]
100465638:      cmp w12, #0xef
10046563c:      b.hi    0x100465654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x55c>
100465640:      sub x10, x10, #0x1
100465644:      add x12, x9, x10
100465648:      cmn x12, #0x1
10046564c:      b.ne    0x100465634 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x53c>
100465650:      b   0x100465660 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x568>
100465654:      neg x10, x10
100465658:      cmp x10, x9
10046565c:      b.ls    0x1004655b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4bc>
100465660:      cmn w3, #0x3
100465664:      b.hi    0x100465684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x58c>
100465668:      mov w9, #0x0                ; =0
10046566c:      add w10, w8, #0x2
100465670:      add w11, w3, #0x2
100465674:      stp w8, w10, [x0, #0x4]
100465678:      str w11, [x0, #0xc]
10046567c:      str w9, [x0]
100465680:      b   0x1004655bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x4c4>
100465684:      mov w9, #0x2                ; =2
100465688:      b   0x10046567c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
10046568c:      adrp    x2, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100465690:      add x2, x2, #0xe20
100465694:      mov w0, #0x5                ; =5
100465698:      mov w1, #0x5                ; =5
10046569c:      bl  0x100c89ecc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
