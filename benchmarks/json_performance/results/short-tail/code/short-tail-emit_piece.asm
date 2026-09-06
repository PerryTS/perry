/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-tail-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
1008d62a0:      sub sp, sp, #0x50
1008d62a4:      stp x22, x21, [sp, #0x20]
1008d62a8:      stp x20, x19, [sp, #0x30]
1008d62ac:      stp x29, x30, [sp, #0x40]
1008d62b0:      add x29, sp, #0x40
1008d62b4:      ldr w8, [x0]
1008d62b8:      cbz w8, 0x1008d6358 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
1008d62bc:      cmp w8, #0x1
1008d62c0:      b.ne    0x1008d63dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
1008d62c4:      ldr w8, [x0, #0x4]
1008d62c8:      strb    wzr, [sp, #0x4]
1008d62cc:      str wzr, [sp]
1008d62d0:      and x9, x1, #0xffff000000000000
1008d62d4:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1008d62d8:      cmp x9, x10
1008d62dc:      b.eq    0x1008d6404 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x164>
1008d62e0:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1008d62e4:      cmp x9, x10
1008d62e8:      b.ne    0x1008d6560 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
1008d62ec:      ubfx    x9, x1, #40, #8
1008d62f0:      cbz x9, 0x1008d6340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1008d62f4:      strb    w1, [sp]
1008d62f8:      cmp x9, #0x1
1008d62fc:      b.eq    0x1008d6340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1008d6300:      lsr x10, x1, #8
1008d6304:      strb    w10, [sp, #0x1]
1008d6308:      cmp x9, #0x2
1008d630c:      b.eq    0x1008d6340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1008d6310:      lsr x10, x1, #16
1008d6314:      strb    w10, [sp, #0x2]
1008d6318:      cmp x9, #0x3
1008d631c:      b.eq    0x1008d6340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1008d6320:      lsr x10, x1, #24
1008d6324:      strb    w10, [sp, #0x3]
1008d6328:      cmp x9, #0x4
1008d632c:      b.eq    0x1008d6340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1008d6330:      lsr x10, x1, #32
1008d6334:      strb    w10, [sp, #0x4]
1008d6338:      cmp x9, #0x5
1008d633c:      b.ne    0x1008d6590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1008d6340:      mov x9, sp
1008d6344:      mov w10, #0x22              ; =34
1008d6348:      strb    w10, [x2]
1008d634c:      mov w11, #0x1               ; =1
1008d6350:      cbnz    w8, 0x1008d6420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x180>
1008d6354:      b   0x1008d6510 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1008d6358:      ldr w19, [x0, #0x4]
1008d635c:      strb    wzr, [sp, #0x4]
1008d6360:      str wzr, [sp]
1008d6364:      and x8, x1, #0xffff000000000000
1008d6368:      mov x9, #0x7fff000000000000 ; =9223090561878065152
1008d636c:      cmp x8, x9
1008d6370:      b.eq    0x1008d651c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
1008d6374:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
1008d6378:      cmp x8, x9
1008d637c:      b.ne    0x1008d6578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
1008d6380:      ubfx    x8, x1, #40, #8
1008d6384:      cbz x8, 0x1008d63d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1008d6388:      strb    w1, [sp]
1008d638c:      cmp x8, #0x1
1008d6390:      b.eq    0x1008d63d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1008d6394:      lsr x9, x1, #8
1008d6398:      strb    w9, [sp, #0x1]
1008d639c:      cmp x8, #0x2
1008d63a0:      b.eq    0x1008d63d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1008d63a4:      lsr x9, x1, #16
1008d63a8:      strb    w9, [sp, #0x2]
1008d63ac:      cmp x8, #0x3
1008d63b0:      b.eq    0x1008d63d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1008d63b4:      lsr x9, x1, #24
1008d63b8:      strb    w9, [sp, #0x3]
1008d63bc:      cmp x8, #0x4
1008d63c0:      b.eq    0x1008d63d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
1008d63c4:      lsr x9, x1, #32
1008d63c8:      strb    w9, [sp, #0x4]
1008d63cc:      cmp x8, #0x5
1008d63d0:      b.ne    0x1008d6590 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1008d63d4:      mov x1, sp
1008d63d8:      b   0x1008d6528 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x288>
1008d63dc:      ldur    q0, [x0, #0x8]
1008d63e0:      ldur    q1, [x0, #0x18]
1008d63e4:      stp q0, q1, [sp]
1008d63e8:      ldr w19, [x0, #0x4]
1008d63ec:      mov x1, sp
1008d63f0:      mov x0, x2
1008d63f4:      mov x2, x19
1008d63f8:      bl  0x100cd43ac <_writev+0x100cd43ac>
1008d63fc:      mov x0, x19
1008d6400:      b   0x1008d654c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1008d6404:      ands    x9, x1, #0xffffffffffff
1008d6408:      add x9, x9, #0x14
1008d640c:      csel    x9, xzr, x9, eq
1008d6410:      mov w10, #0x22              ; =34
1008d6414:      strb    w10, [x2]
1008d6418:      mov w11, #0x1               ; =1
1008d641c:      cbz w8, 0x1008d6510 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1008d6420:      mov w12, #0x5c              ; =92
1008d6424:      mov w13, #0x3075            ; =12405
1008d6428:      mov w14, #0x30              ; =48
1008d642c:      adrp    x15, 0x100e03000 <_anon.2faa2ae5fa73ebf7e6102d50cd6666c0.1847+0xc23>
1008d6430:      add x15, x15, #0x878
1008d6434:      b   0x1008d6450 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
1008d6438:      mov w16, #0x62              ; =98
1008d643c:      strb    w16, [x17, #0x1]
1008d6440:      mov w16, #0x2               ; =2
1008d6444:      add x11, x16, x11
1008d6448:      subs    x8, x8, #0x1
1008d644c:      b.eq    0x1008d6510 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1008d6450:      ldrb    w16, [x9], #0x1
1008d6454:      cmp w16, #0x20
1008d6458:      b.lo    0x1008d646c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
1008d645c:      cmp w16, #0x5c
1008d6460:      b.eq    0x1008d646c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
1008d6464:      cmp w16, #0x22
1008d6468:      b.ne    0x1008d64f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x254>
1008d646c:      add x17, x2, x11
1008d6470:      strb    w12, [x17]
1008d6474:      cmp w16, #0xb
1008d6478:      b.le    0x1008d649c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
1008d647c:      cmp w16, #0x21
1008d6480:      b.gt    0x1008d64bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
1008d6484:      cmp w16, #0xc
1008d6488:      b.eq    0x1008d6500 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x260>
1008d648c:      cmp w16, #0xd
1008d6490:      b.ne    0x1008d64cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
1008d6494:      mov w16, #0x72              ; =114
1008d6498:      b   0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d649c:      cmp w16, #0x8
1008d64a0:      b.eq    0x1008d6438 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x198>
1008d64a4:      cmp w16, #0x9
1008d64a8:      b.eq    0x1008d6508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x268>
1008d64ac:      cmp w16, #0xa
1008d64b0:      b.ne    0x1008d64cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
1008d64b4:      mov w16, #0x6e              ; =110
1008d64b8:      b   0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d64bc:      cmp w16, #0x22
1008d64c0:      b.eq    0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d64c4:      cmp w16, #0x5c
1008d64c8:      b.eq    0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d64cc:      sturh   w13, [x17, #0x1]
1008d64d0:      strb    w14, [x17, #0x3]
1008d64d4:      lsr x0, x16, #4
1008d64d8:      ldrb    w0, [x15, x0]
1008d64dc:      strb    w0, [x17, #0x4]
1008d64e0:      and x16, x16, #0xf
1008d64e4:      ldrb    w16, [x15, x16]
1008d64e8:      strb    w16, [x17, #0x5]
1008d64ec:      mov w16, #0x6               ; =6
1008d64f0:      b   0x1008d6444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1008d64f4:      strb    w16, [x2, x11]
1008d64f8:      mov w16, #0x1               ; =1
1008d64fc:      b   0x1008d6444 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1008d6500:      mov w16, #0x66              ; =102
1008d6504:      b   0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d6508:      mov w16, #0x74              ; =116
1008d650c:      b   0x1008d643c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1008d6510:      strb    w10, [x2, x11]
1008d6514:      add x0, x11, #0x1
1008d6518:      b   0x1008d654c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1008d651c:      ands    x8, x1, #0xffffffffffff
1008d6520:      add x8, x8, #0x14
1008d6524:      csel    x1, xzr, x8, eq
1008d6528:      mov w20, #0x22              ; =34
1008d652c:      mov x0, x2
1008d6530:      strb    w20, [x0], #0x1
1008d6534:      mov x21, x2
1008d6538:      mov x2, x19
1008d653c:      bl  0x100cd43ac <_writev+0x100cd43ac>
1008d6540:      add x8, x21, x19
1008d6544:      strb    w20, [x8, #0x1]
1008d6548:      add x0, x19, #0x2
1008d654c:      ldp x29, x30, [sp, #0x40]
1008d6550:      ldp x20, x19, [sp, #0x30]
1008d6554:      ldp x22, x21, [sp, #0x20]
1008d6558:      add sp, sp, #0x50
1008d655c:      ret
1008d6560:      adrp    x0, 0x100e09000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_KOI8_R+0x58>
1008d6564:      add x0, x0, #0xe8e
1008d6568:      adrp    x2, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d656c:      add x2, x2, #0xf58
1008d6570:      mov w1, #0x20               ; =32
1008d6574:      bl  0x100c83d00 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1008d6578:      adrp    x0, 0x100e09000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_KOI8_R+0x58>
1008d657c:      add x0, x0, #0xe76
1008d6580:      adrp    x2, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6584:      add x2, x2, #0xf40
1008d6588:      mov w1, #0x18               ; =24
1008d658c:      bl  0x100c83d00 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
1008d6590:      adrp    x2, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008d6594:      add x2, x2, #0x968
1008d6598:      mov w0, #0x5                ; =5
1008d659c:      mov w1, #0x5                ; =5
1008d65a0:      bl  0x100c83dcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
