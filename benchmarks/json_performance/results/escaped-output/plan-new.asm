
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-output-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010021420c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new>:
10021420c:      sub sp, sp, #0x50
100214210:      stp x22, x21, [sp, #0x20]
100214214:      stp x20, x19, [sp, #0x30]
100214218:      stp x29, x30, [sp, #0x40]
10021421c:      add x29, sp, #0x40
100214220:      mov x21, x3
100214224:      mov x20, x2
100214228:      mov x22, x1
10021422c:      mov x19, x0
100214230:      add x8, sp, #0x8
100214234:      mov x0, x1
100214238:      mov x1, x2
10021423c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100214240:      ldr x8, [sp, #0x8]
100214244:      cbnz    x8, 0x1002142c8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xbc>
100214248:      mov x8, #0x0                ; =0
10021424c:      cbz x20, 0x1002142d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xc4>
100214250:      mov w9, #0x37               ; =55
100214254:      mov w10, #0x5c              ; =92
100214258:      mov x11, x20
10021425c:      b   0x100214278 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x6c>
100214260:      mov x12, #0x0               ; =0
100214264:      mov w13, #0x1               ; =1
100214268:      add x8, x8, x12
10021426c:      add x8, x13, x8
100214270:      subs    x11, x11, #0x1
100214274:      b.eq    0x1002142bc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xb0>
100214278:      ldrb    w13, [x22], #0x1
10021427c:      sub w12, w13, #0x8
100214280:      cmp w12, #0x6
100214284:      cset    w14, lo
100214288:      lsr w12, w9, w12
10021428c:      and w12, w14, w12
100214290:      tbnz    w12, #0x0, 0x100214260 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x54>
100214294:      cmp w13, #0x20
100214298:      b.hs    0x1002142a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x9c>
10021429c:      mov w13, #0x1               ; =1
1002142a0:      mov w12, #0x4               ; =4
1002142a4:      b   0x100214268 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x5c>
1002142a8:      mov x12, #0x0               ; =0
1002142ac:      cmp w13, #0x22
1002142b0:      ccmp    w13, w10, #0x4, ne
1002142b4:      cset    w13, eq
1002142b8:      b   0x100214268 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0x5c>
1002142bc:      mov w9, #-0x3               ; =-3
1002142c0:      cmp x8, x9
1002142c4:      b.ls    0x1002142d0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xc4>
1002142c8:      mov w8, #0x0                ; =0
1002142cc:      b   0x1002142f0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xe4>
1002142d0:      add w9, w8, #0x2
1002142d4:      adds    w8, w9, w20
1002142d8:      b.hs    0x1002142c8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xbc>
1002142dc:      adds    w9, w9, w21
1002142e0:      b.hs    0x1002142c8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_escaped_outputNtB2_4Plan3new+0xbc>
1002142e4:      stp w20, w8, [x19, #0x4]
1002142e8:      mov w8, #0x1                ; =1
1002142ec:      str w9, [x19, #0xc]
1002142f0:      str w8, [x19]
1002142f4:      ldp x29, x30, [sp, #0x40]
1002142f8:      ldp x20, x19, [sp, #0x30]
1002142fc:      ldp x22, x21, [sp, #0x20]
100214300:      add sp, sp, #0x50
100214304:      ret
