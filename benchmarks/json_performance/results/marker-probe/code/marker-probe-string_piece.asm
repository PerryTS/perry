/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>:
1007500d0:      sub sp, sp, #0x40
1007500d4:      stp x20, x19, [sp, #0x20]
1007500d8:      stp x29, x30, [sp, #0x30]
1007500dc:      add x29, sp, #0x30
1007500e0:      strb    wzr, [sp, #0xc]
1007500e4:      str wzr, [sp, #0x8]
1007500e8:      and x9, x1, #0xffff000000000000
1007500ec:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1007500f0:      cmp x9, x8
1007500f4:      b.eq    0x1007502d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x204>
1007500f8:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1007500fc:      cmp x9, x8
100750100:      b.ne    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750104:      ubfx    x8, x1, #40, #8
100750108:      cbz x8, 0x100750158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
10075010c:      strb    w1, [sp, #0x8]
100750110:      cmp x8, #0x1
100750114:      b.eq    0x100750158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100750118:      lsr x10, x1, #8
10075011c:      strb    w10, [sp, #0x9]
100750120:      cmp x8, #0x2
100750124:      b.eq    0x100750158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100750128:      lsr x10, x1, #16
10075012c:      strb    w10, [sp, #0xa]
100750130:      cmp x8, #0x3
100750134:      b.eq    0x100750158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100750138:      lsr x10, x1, #24
10075013c:      strb    w10, [sp, #0xb]
100750140:      cmp x8, #0x4
100750144:      b.eq    0x100750158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x88>
100750148:      lsr x10, x1, #32
10075014c:      strb    w10, [sp, #0xc]
100750150:      cmp x8, #0x5
100750154:      b.ne    0x1007506e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x610>
100750158:      mov x10, x1
10075015c:      add x1, sp, #0x8
100750160:      mov w2, w8
100750164:      and w11, w8, #0xfffffffc
100750168:      cmp w11, #0x4
10075016c:      b.ne    0x100750300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x230>
100750170:      ldr w11, [x1]
100750174:      tbz w2, #0x1, 0x100750180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xb0>
100750178:      ldrh    w12, [x1, #0x4]
10075017c:      orr x11, x11, x12, lsl #32
100750180:      tbz w2, #0x0, 0x100750198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xc8>
100750184:      sub x12, x2, #0x1
100750188:      ldrb    w13, [x1, x12]
10075018c:      lsl x12, x12, #3
100750190:      lsl x12, x13, x12
100750194:      orr x11, x12, x11
100750198:      lsl x12, x2, #3
10075019c:      mov x13, #0x2020202020202020 ; =2314885530818453536
1007501a0:      lsl x12, x13, x12
1007501a4:      orr x12, x11, x12
1007501a8:      eor x13, x12, #0x2222222222222222
1007501ac:      mov x14, #-0x101010101010102 ; =-72340172838076674
1007501b0:      movk    x14, #0xfeff
1007501b4:      mov x15, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
1007501b8:      orr x15, x15, #0x4444444444444444
1007501bc:      eor x15, x12, x15
1007501c0:      add x15, x15, x14
1007501c4:      mov x16, #-0x2020202020202021 ; =-2314885530818453537
1007501c8:      movk    x16, #0xdfe0
1007501cc:      add x16, x12, x16
1007501d0:      orr x15, x15, x16
1007501d4:      add x13, x13, x14
1007501d8:      orr x13, x15, x13
1007501dc:      bic x13, x13, x11
1007501e0:      mov x15, #-0x3333333333333334 ; =-3689348814741910324
1007501e4:      orr x15, x15, #0xe1e1e1e1e1e1e1e1
1007501e8:      eor x12, x12, x15
1007501ec:      add x12, x12, x14
1007501f0:      and x11, x12, x11
1007501f4:      orr x11, x13, x11
1007501f8:      tst x11, #0x8080808080808080
1007501fc:      cset    w11, ne
100750200:      mov x12, #0x7fff000000000000 ; =9223090561878065152
100750204:      cmp x9, x12
100750208:      b.eq    0x1007505f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x520>
10075020c:      cmp w8, #0x40
100750210:      b.hs    0x1007503c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2f0>
100750214:      ands    x9, x2, #0x38
100750218:      b.eq    0x10075023c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x16c>
10075021c:      and x10, x2, #0x38
100750220:      neg x10, x10
100750224:      mov x12, x1
100750228:      ldr x13, [x12], #0x8
10075022c:      tst x13, #0x8080808080808080
100750230:      b.ne    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750234:      adds    x10, x10, #0x8
100750238:      b.ne    0x100750228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x158>
10075023c:      mov x3, x8
100750240:      and x10, x2, #0x7
100750244:      cbz x10, 0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
100750248:      add x9, x1, x9
10075024c:      ldrsb   w12, [x9]
100750250:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750254:      mov x3, x8
100750258:      cmp x10, #0x1
10075025c:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
100750260:      ldrsb   w12, [x9, #0x1]
100750264:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750268:      mov x3, x8
10075026c:      cmp x10, #0x2
100750270:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
100750274:      ldrsb   w12, [x9, #0x2]
100750278:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
10075027c:      mov x3, x8
100750280:      cmp x10, #0x3
100750284:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
100750288:      ldrsb   w12, [x9, #0x3]
10075028c:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750290:      mov x3, x8
100750294:      cmp x10, #0x4
100750298:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10075029c:      ldrsb   w12, [x9, #0x4]
1007502a0:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007502a4:      mov x3, x8
1007502a8:      cmp x10, #0x5
1007502ac:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
1007502b0:      ldrsb   w12, [x9, #0x5]
1007502b4:      tbnz    w12, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007502b8:      mov x3, x8
1007502bc:      cmp x10, #0x6
1007502c0:      b.eq    0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
1007502c4:      ldrsb   w9, [x9, #0x6]
1007502c8:      mov x3, x8
1007502cc:      tbz w9, #0x1f, 0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
1007502d0:      b   0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007502d4:      ands    x11, x1, #0xffffffffffff
1007502d8:      b.eq    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007502dc:      ldr w8, [x11, #0x4]
1007502e0:      cmn w8, #0x3
1007502e4:      b.hi    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007502e8:      mov x10, x1
1007502ec:      add x1, x11, #0x14
1007502f0:      mov w2, w8
1007502f4:      and w11, w8, #0xfffffffc
1007502f8:      cmp w11, #0x4
1007502fc:      b.eq    0x100750170 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0xa0>
100750300:      cmp w8, #0x10
100750304:      b.lo    0x100750358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x288>
100750308:      mov w11, #0x10              ; =16
10075030c:      movi.16b    v0, #0x22
100750310:      movi.16b    v1, #0x5c
100750314:      movi.16b    v2, #0x20
100750318:      mov x12, x1
10075031c:      movi.16b    v3, #0xed
100750320:      ldr q4, [x12], #0x10
100750324:      cmeq.16b    v5, v4, v0
100750328:      cmeq.16b    v6, v4, v1
10075032c:      orr.16b v5, v6, v5
100750330:      cmhi.16b    v6, v2, v4
100750334:      cmeq.16b    v4, v4, v3
100750338:      orr.16b v4, v4, v6
10075033c:      orr.16b v4, v4, v5
100750340:      addp.2d d4, v4
100750344:      fmov    x13, d4
100750348:      cbnz    x13, 0x1007505d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x508>
10075034c:      add x11, x11, #0x10
100750350:      cmp x11, x2
100750354:      b.ls    0x100750320 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x250>
100750358:      and x12, x2, #0xfffffff0
10075035c:      add x13, x1, x12
100750360:      tbnz    w2, #0x3, 0x10075044c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x37c>
100750364:      tst x2, #0x7
100750368:      b.eq    0x1007503ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
10075036c:      and x11, x2, #0x8
100750370:      add x13, x13, x11
100750374:      sub x11, x2, x11
100750378:      sub x12, x11, x12
10075037c:      ldrb    w14, [x13], #0x1
100750380:      mov w11, #0x1               ; =1
100750384:      cmp w14, #0x22
100750388:      b.eq    0x1007505e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10075038c:      cmp w14, #0x5c
100750390:      b.eq    0x1007505e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
100750394:      cmp w14, #0x20
100750398:      b.lo    0x1007505e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
10075039c:      cmp w14, #0xed
1007503a0:      b.eq    0x1007505e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
1007503a4:      subs    x12, x12, #0x1
1007503a8:      b.ne    0x10075037c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2ac>
1007503ac:      mov w11, #0x0               ; =0
1007503b0:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1007503b4:      cmp x9, x12
1007503b8:      b.ne    0x10075020c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1007503bc:      b   0x1007505f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x520>
1007503c0:      and x9, x2, #0xffffffc0
1007503c4:      add x9, x1, x9
1007503c8:      mov x10, x1
1007503cc:      ldp q0, q1, [x10]
1007503d0:      ldp q2, q3, [x10, #0x20]
1007503d4:      orr.16b v0, v1, v0
1007503d8:      orr.16b v1, v2, v3
1007503dc:      orr.16b v0, v0, v1
1007503e0:      umaxv.16b   b0, v0
1007503e4:      fmov    w12, s0
1007503e8:      tbnz    w12, #0x7, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007503ec:      add x10, x10, #0x40
1007503f0:      cmp x10, x9
1007503f4:      b.ne    0x1007503cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2fc>
1007503f8:      ands    x10, x2, #0x30
1007503fc:      b.eq    0x100750420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x350>
100750400:      mov x12, x10
100750404:      mov x13, x9
100750408:      ldr q0, [x13], #0x10
10075040c:      umaxv.16b   b0, v0
100750410:      fmov    w14, s0
100750414:      tbnz    w14, #0x7, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750418:      subs    x12, x12, #0x10
10075041c:      b.ne    0x100750408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x338>
100750420:      mov x3, x8
100750424:      and x12, x2, #0xf
100750428:      cbz x12, 0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10075042c:      add x9, x9, x10
100750430:      ldrsb   w10, [x9]
100750434:      tbnz    w10, #0x1f, 0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
100750438:      add x9, x9, #0x1
10075043c:      subs    x12, x12, #0x1
100750440:      b.ne    0x100750430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x360>
100750444:      mov x3, x8
100750448:      b   0x1007505f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x528>
10075044c:      mov x14, #0x101010101010101 ; =72340172838076673
100750450:      movk    x14, #0x100
100750454:      ldr x11, [x13]
100750458:      eor x15, x11, #0x2222222222222222
10075045c:      sub x15, x14, x15
100750460:      orr x16, x15, x11
100750464:      mov x15, #-0x7f7f7f7f7f7f7f80 ; =-9187201950435737472
100750468:      bics    xzr, x15, x16
10075046c:      b.ne    0x1007504c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
100750470:      mov x16, #0x1c1c1c1c1c1c1c1c ; =2025524839466146844
100750474:      orr x16, x16, #0x4444444444444444
100750478:      eor x16, x11, x16
10075047c:      sub x14, x14, x16
100750480:      orr x14, x14, x11
100750484:      bics    xzr, x15, x14
100750488:      b.ne    0x1007504c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
10075048c:      mov x14, #0x2020202020202020 ; =2314885530818453536
100750490:      movk    x14, #0x201f
100750494:      sub x14, x14, x11
100750498:      orr x14, x14, x11
10075049c:      bics    xzr, x15, x14
1007504a0:      b.ne    0x1007504c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x3f8>
1007504a4:      mov x14, #-0x3333333333333334 ; =-3689348814741910324
1007504a8:      orr x14, x14, #0xe1e1e1e1e1e1e1e1
1007504ac:      eor x14, x11, x14
1007504b0:      mov x15, #-0x101010101010102 ; =-72340172838076674
1007504b4:      movk    x15, #0xfeff
1007504b8:      add x14, x14, x15
1007504bc:      and x14, x11, x14
1007504c0:      tst x14, #0x8080808080808080
1007504c4:      b.eq    0x100750364 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x294>
1007504c8:      and w12, w11, #0xff
1007504cc:      cmp w12, #0x22
1007504d0:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007504d4:      cmp w12, #0x5c
1007504d8:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007504dc:      cmp w12, #0x20
1007504e0:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007504e4:      cmp w12, #0xed
1007504e8:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007504ec:      ubfx    w12, w11, #8, #8
1007504f0:      cmp w12, #0x22
1007504f4:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007504f8:      cmp w12, #0x5c
1007504fc:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750500:      cmp w12, #0xed
100750504:      mov w13, #0x20              ; =32
100750508:      ccmp    w12, w13, #0x0, ne
10075050c:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750510:      ubfx    w12, w11, #16, #8
100750514:      cmp w12, #0x22
100750518:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10075051c:      cmp w12, #0x5c
100750520:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750524:      cmp w12, #0xed
100750528:      ccmp    w12, w13, #0x0, ne
10075052c:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750530:      lsr w12, w11, #24
100750534:      cmp w12, #0x22
100750538:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10075053c:      cmp w12, #0x5c
100750540:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750544:      cmp w12, #0xed
100750548:      ccmp    w12, w13, #0x0, ne
10075054c:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750550:      ubfx    x12, x11, #32, #8
100750554:      cmp w12, #0x22
100750558:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10075055c:      cmp w12, #0x5c
100750560:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750564:      cmp w12, #0xed
100750568:      ccmp    w12, w13, #0x0, ne
10075056c:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750570:      ubfx    x12, x11, #40, #8
100750574:      cmp w12, #0x22
100750578:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10075057c:      cmp w12, #0x5c
100750580:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750584:      cmp w12, #0xed
100750588:      ccmp    w12, w13, #0x0, ne
10075058c:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
100750590:      ubfx    x12, x11, #48, #8
100750594:      cmp w12, #0x22
100750598:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
10075059c:      cmp w12, #0x5c
1007505a0:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007505a4:      cmp w12, #0xed
1007505a8:      ccmp    w12, w13, #0x0, ne
1007505ac:      b.lo    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007505b0:      lsr x12, x11, #56
1007505b4:      cmp w12, #0x22
1007505b8:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007505bc:      cmp w12, #0x5c
1007505c0:      b.eq    0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007505c4:      lsr x11, x11, #61
1007505c8:      cmp x12, #0xed
1007505cc:      ccmp    x11, #0x0, #0x4, ne
1007505d0:      b.ne    0x1007503ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x2dc>
1007505d4:      b   0x1007505e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x510>
1007505d8:      fmov    x11, d4
1007505dc:      cbz x11, 0x1007505e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x514>
1007505e0:      mov w11, #0x1               ; =1
1007505e4:      mov x12, #0x7fff000000000000 ; =9223090561878065152
1007505e8:      cmp x9, x12
1007505ec:      b.ne    0x10075020c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x13c>
1007505f0:      and x9, x10, #0xffffffffffff
1007505f4:      ldr w3, [x9]
1007505f8:      tbz w11, #0x0, 0x10075062c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x55c>
1007505fc:      mov x19, x0
100750600:      add x0, sp, #0x10
100750604:      bl  0x1006fdd88 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>
100750608:      ldr w8, [sp, #0x10]
10075060c:      tbz w8, #0x0, 0x100750664 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x594>
100750610:      ldur    x8, [sp, #0x14]
100750614:      stur    x8, [x19, #0x4]
100750618:      ldr w8, [sp, #0x1c]
10075061c:      str w8, [x19, #0xc]
100750620:      mov w8, #0x1                ; =1
100750624:      str w8, [x19]
100750628:      b   0x100750654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
10075062c:      mov w9, #0x3                ; =3
100750630:      cmp x2, #0x3
100750634:      csel    x9, x2, x9, lo
100750638:      cbz w8, 0x1007506cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
10075063c:      add x10, x1, x2
100750640:      ldurb   w10, [x10, #-0x1]
100750644:      cmp w10, #0xbf
100750648:      b.ls    0x100750670 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5a0>
10075064c:      mov w8, #-0x1               ; =-1
100750650:      str w8, [x0]
100750654:      ldp x29, x30, [sp, #0x30]
100750658:      ldp x20, x19, [sp, #0x20]
10075065c:      add sp, sp, #0x40
100750660:      ret
100750664:      mov w8, #-0x1               ; =-1
100750668:      str w8, [x19]
10075066c:      b   0x100750654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
100750670:      cmp w8, #0x1
100750674:      b.eq    0x1007506cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
100750678:      add x10, x1, x2
10075067c:      ldurb   w10, [x10, #-0x2]
100750680:      cmp w10, #0xdf
100750684:      b.ls    0x100750690 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5c0>
100750688:      mov w10, #0x2               ; =2
10075068c:      b   0x1007506c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f4>
100750690:      cmp w8, #0x2
100750694:      b.eq    0x1007506cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
100750698:      add x11, x1, x2
10075069c:      mov x10, #-0x3              ; =-3
1007506a0:      ldrb    w12, [x11, x10]
1007506a4:      cmp w12, #0xef
1007506a8:      b.hi    0x1007506c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5f0>
1007506ac:      sub x10, x10, #0x1
1007506b0:      add x12, x9, x10
1007506b4:      cmn x12, #0x1
1007506b8:      b.ne    0x1007506a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5d0>
1007506bc:      b   0x1007506cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x5fc>
1007506c0:      neg x10, x10
1007506c4:      cmp x10, x9
1007506c8:      b.ls    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007506cc:      cmn w3, #0x3
1007506d0:      b.hi    0x10075064c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x57c>
1007506d4:      stp wzr, w8, [x0]
1007506d8:      str w3, [x0, #0x8]
1007506dc:      b   0x100750654 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece+0x584>
1007506e0:      adrp    x2, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
1007506e4:      add x2, x2, #0xf10
1007506e8:      mov w0, #0x5                ; =5
1007506ec:      mov w1, #0x5                ; =5
1007506f0:      bl  0x100c8d38c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
