
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-count-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100435544 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>:
100435544:      sub sp, sp, #0x50
100435548:      stp x22, x21, [sp, #0x20]
10043554c:      stp x20, x19, [sp, #0x30]
100435550:      stp x29, x30, [sp, #0x40]
100435554:      add x29, sp, #0x40
100435558:      mov x21, x3
10043555c:      mov x20, x2
100435560:      mov x22, x1
100435564:      mov x19, x0
100435568:      add x8, sp, #0x8
10043556c:      mov x0, x1
100435570:      mov x1, x2
100435574:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100435578:      ldr x8, [sp, #0x8]
10043557c:      cbnz    x8, 0x1004356ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x168>
100435580:      cmp x20, #0x10
100435584:      b.lo    0x1004355f0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xac>
100435588:      mov x8, #0x0                ; =0
10043558c:      adrp    x9, 0x100dd0000 <_anon.88ed17a1392924f08814ef64693a15d8.1051+0x4e6>
100435590:      ldr q0, [x9, #0xf40]
100435594:      movi.16b    v1, #0x5
100435598:      movi.16b    v2, #0x22
10043559c:      movi.16b    v3, #0x5c
1004355a0:      movi.16b    v4, #0x1
1004355a4:      mov x9, x22
1004355a8:      mov x10, x20
1004355ac:      ldr q5, [x9], #0x10
1004355b0:      tbl.16b v6, { v0, v1 }, v5
1004355b4:      cmeq.16b    v7, v5, v2
1004355b8:      cmeq.16b    v5, v5, v3
1004355bc:      orr.16b v5, v5, v7
1004355c0:      and.16b v5, v5, v4
1004355c4:      orr.16b v5, v6, v5
1004355c8:      addv.16b    b5, v5
1004355cc:      fmov    w11, s5
1004355d0:      add x8, x8, w11, uxtb
1004355d4:      sub x10, x10, #0x10
1004355d8:      cmp x10, #0xf
1004355dc:      b.hi    0x1004355ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x68>
1004355e0:      and x16, x20, #0xfffffff0
1004355e4:      subs    x15, x20, x16
1004355e8:      b.ne    0x100435600 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xbc>
1004355ec:      b   0x1004356a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x15c>
1004355f0:      mov x16, #0x0               ; =0
1004355f4:      mov x8, #0x0                ; =0
1004355f8:      subs    x15, x20, x16
1004355fc:      b.eq    0x1004356a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x15c>
100435600:      add x9, x22, x16
100435604:      adrp    x10, 0x100dd4000 <_anon.152773887b8e060e913a13a302c04959.589+0x3>
100435608:      add x10, x10, #0x718
10043560c:      cmp x15, #0x4
100435610:      b.lo    0x100435688 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x144>
100435614:      mov x12, #0x0               ; =0
100435618:      mov x13, #0x0               ; =0
10043561c:      mov x14, #0x0               ; =0
100435620:      and x11, x20, #0x3
100435624:      sub x15, x15, x11
100435628:      add x9, x9, x15
10043562c:      add x15, x16, x11
100435630:      sub x15, x15, x20
100435634:      add x16, x16, x22
100435638:      add x16, x16, #0x1
10043563c:      ldurb   w17, [x16, #-0x1]
100435640:      ldrb    w0, [x16]
100435644:      ldrb    w1, [x16, #0x1]
100435648:      ldrb    w2, [x16, #0x2]
10043564c:      ldrb    w17, [x10, x17]
100435650:      ldrb    w0, [x10, x0]
100435654:      ldrb    w1, [x10, x1]
100435658:      ldrb    w2, [x10, x2]
10043565c:      add x8, x8, x17
100435660:      add x12, x12, x0
100435664:      add x13, x13, x1
100435668:      add x14, x14, x2
10043566c:      add x16, x16, #0x4
100435670:      adds    x15, x15, #0x4
100435674:      b.ne    0x10043563c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xf8>
100435678:      add x8, x12, x8
10043567c:      add x12, x14, x13
100435680:      add x8, x12, x8
100435684:      cbz x11, 0x1004356a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x15c>
100435688:      add x11, x22, x20
10043568c:      ldrb    w12, [x9], #0x1
100435690:      ldrb    w12, [x10, x12]
100435694:      add x8, x8, x12
100435698:      cmp x9, x11
10043569c:      b.ne    0x10043568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x148>
1004356a0:      mov w9, #-0x3               ; =-3
1004356a4:      cmp x8, x9
1004356a8:      b.ls    0x1004356b4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x170>
1004356ac:      mov w8, #0x0                ; =0
1004356b0:      b   0x1004356d4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x190>
1004356b4:      add w9, w8, #0x2
1004356b8:      adds    w8, w9, w20
1004356bc:      b.hs    0x1004356ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x168>
1004356c0:      adds    w9, w9, w21
1004356c4:      b.hs    0x1004356ac <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x168>
1004356c8:      stp w20, w8, [x19, #0x4]
1004356cc:      mov w8, #0x1                ; =1
1004356d0:      str w9, [x19, #0xc]
1004356d4:      str w8, [x19]
1004356d8:      ldp x29, x30, [sp, #0x40]
1004356dc:      ldp x20, x19, [sp, #0x30]
1004356e0:      ldp x22, x21, [sp, #0x20]
1004356e4:      add sp, sp, #0x50
1004356e8:      ret
