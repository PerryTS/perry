/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-count-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100464460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object>:
100464460:      stp x28, x27, [sp, #-0x60]!
100464464:      stp x26, x25, [sp, #0x10]
100464468:      stp x24, x23, [sp, #0x20]
10046446c:      stp x22, x21, [sp, #0x30]
100464470:      stp x20, x19, [sp, #0x40]
100464474:      stp x29, x30, [sp, #0x50]
100464478:      add x29, sp, #0x50
10046447c:      sub sp, sp, #0x1c0
100464480:      mov x19, x1
100464484:      mov x20, x0
100464488:      movi.2d v0, #0000000000000000
10046448c:      stp q0, q0, [sp, #0x130]
100464490:      add x22, sp, #0xb0
100464494:      mov w8, #0x2                ; =2
100464498:      str w8, [sp, #0x10]
10046449c:      add x23, sp, #0x10
1004644a0:      stur    q0, [sp, #0x14]
1004644a4:      stur    q0, [sp, #0x24]
1004644a8:      mov x9, #0x200000000        ; =8589934592
1004644ac:      stur    x9, [sp, #0x34]
1004644b0:      stur    q0, [sp, #0x3c]
1004644b4:      stur    q0, [sp, #0x4c]
1004644b8:      stur    x9, [sp, #0x5c]
1004644bc:      stur    q0, [sp, #0x64]
1004644c0:      stur    q0, [sp, #0x74]
1004644c4:      stur    x9, [sp, #0x84]
1004644c8:      stur    q0, [sp, #0x8c]
1004644cc:      stur    q0, [x23, #0x8c]
1004644d0:      stp wzr, w8, [sp, #0xac]
1004644d4:      stp wzr, w8, [sp, #0xd4]
1004644d8:      stur    q0, [x23, #0xb4]
1004644dc:      stur    q0, [x23, #0xa4]
1004644e0:      stp wzr, w8, [sp, #0xfc]
1004644e4:      stur    q0, [x23, #0xdc]
1004644e8:      stur    q0, [x23, #0xcc]
1004644ec:      str wzr, [sp, #0x124]
1004644f0:      stur    q0, [x22, #0x64]
1004644f4:      stur    q0, [x22, #0x54]
1004644f8:      str x8, [sp, #0x128]
1004644fc:      ldr w21, [x0, #0x4]
100464500:      adrp    x26, 0x101125000 <__MergedGlobals+0x48>
100464504:      add x26, x26, #0x3b4
100464508:      ldr w24, [x26]
10046450c:      adrp    x25, 0x101124000 <_perry_global_baseline_worker_ts__1>
100464510:      add x25, x25, #0x478
100464514:      cmp w24, #0x300
100464518:      b.hs    0x100464b1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6bc>
10046451c:      ldr x8, [x25]
100464520:      cmn x8, #0x1
100464524:      b.eq    0x100464b0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ac>
100464528:      mrs x9, TPIDRRO_EL0
10046452c:      and x9, x9, #0xfffffffffffffff8
100464530:      ldr x0, [x9, x8, lsl #3]
100464534:      cbz x0, 0x100464b0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ac>
100464538:      add x8, x0, x24, lsl #3
10046453c:      ldr x0, [x8, #0x1e8]
100464540:      cbz x0, 0x100464b1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6bc>
100464544:      ldr x0, [x0]
100464548:      cbz x0, 0x100464b30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d0>
10046454c:      ldr x8, [x0, #0x5190]
100464550:      ubfx    x9, x21, #15, #15
100464554:      ubfx    x10, x21, #5, #10
100464558:      and x11, x21, #0x1f
10046455c:      ldr x8, [x8, x9, lsl #3]
100464560:      ldr x8, [x8, x10, lsl #3]
100464564:      lsl x9, x11, #5
100464568:      ldr x24, [x8, x9]
10046456c:      ldr x1, [x24, #0x8]
100464570:      sub x0, x29, #0x90
100464574:      bl  0x100465274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
100464578:      ldur    w8, [x29, #-0x90]
10046457c:      cmn w8, #0x1
100464580:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464584:      ldur    x8, [x29, #-0x70]
100464588:      ldp q1, q0, [x29, #-0x90]
10046458c:      stp q1, q0, [sp, #0x10]
100464590:      str x8, [sp, #0x30]
100464594:      ldr x1, [x20, #0x10]
100464598:      sub x0, x29, #0x90
10046459c:      bl  0x100464fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1004645a0:      ldur    w8, [x29, #-0x90]
1004645a4:      cmn w8, #0x1
1004645a8:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
1004645ac:      ldur    x8, [x29, #-0x70]
1004645b0:      ldp q1, q0, [x29, #-0x90]
1004645b4:      stp q1, q0, [sp, #0xb0]
1004645b8:      str x8, [sp, #0xd0]
1004645bc:      ldp w10, w8, [sp, #0x10]
1004645c0:      ldr w9, [sp, #0x18]
1004645c4:      cbz w10, 0x1004645f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x190>
1004645c8:      cmp w10, #0x1
1004645cc:      b.ne    0x100464618 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1b8>
1004645d0:      ldr w8, [sp, #0x1c]
1004645d4:      ldp w12, w10, [sp, #0xb0]
1004645d8:      ldr w11, [sp, #0xb8]
1004645dc:      cbz w12, 0x100464608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a8>
1004645e0:      cmp w12, #0x1
1004645e4:      b.ne    0x10046462c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1cc>
1004645e8:      ldr w10, [sp, #0xbc]
1004645ec:      b   0x100464630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1d0>
1004645f0:      add w10, w8, #0x2
1004645f4:      add w8, w9, #0x2
1004645f8:      mov x9, x10
1004645fc:      ldp w12, w10, [sp, #0xb0]
100464600:      ldr w11, [sp, #0xb8]
100464604:      cbnz    w12, 0x1004645e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x180>
100464608:      add w12, w10, #0x2
10046460c:      add w10, w11, #0x2
100464610:      mov x11, x12
100464614:      b   0x100464630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1d0>
100464618:      mov x9, x8
10046461c:      ldp w12, w10, [sp, #0xb0]
100464620:      ldr w11, [sp, #0xb8]
100464624:      cbnz    w12, 0x1004645e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x180>
100464628:      b   0x100464608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a8>
10046462c:      mov x11, x10
100464630:      cmn w9, #0x3
100464634:      b.hi    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464638:      add w9, w9, #0x2
10046463c:      adds    w9, w11, w9
100464640:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464644:      mov x0, #0x0                ; =0
100464648:      adds    w21, w9, #0x1
10046464c:      b.hs    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464650:      cmn w8, #0x3
100464654:      b.hi    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464658:      mov x0, #0x0                ; =0
10046465c:      add w8, w8, #0x2
100464660:      adds    w28, w10, w8
100464664:      b.hs    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464668:      cmn w28, #0x1
10046466c:      b.eq    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464670:      add w27, w28, #0x1
100464674:      cmp x19, #0x1
100464678:      b.ne    0x1004646b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x258>
10046467c:      ldr x8, [x25]
100464680:      cmn x8, #0x1
100464684:      b.eq    0x10046472c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
100464688:      mrs x9, TPIDRRO_EL0
10046468c:      and x9, x9, #0xfffffffffffffff8
100464690:      ldr x8, [x9, x8, lsl #3]
100464694:      cbz x8, 0x10046472c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
100464698:      ldr x8, [x8, #0x19e8]
10046469c:      cbz x8, 0x100464b38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d8>
1004646a0:      ldr x9, [x8]
1004646a4:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004646a8:      cmp x9, x10
1004646ac:      b.hs    0x100464ec8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa68>
1004646b0:      ldr x22, [x8, #0x18]
1004646b4:      b   0x100464758 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2f8>
1004646b8:      ldr x1, [x24, #0x10]
1004646bc:      sub x0, x29, #0x90
1004646c0:      bl  0x100465274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1004646c4:      ldur    w8, [x29, #-0x90]
1004646c8:      cmn w8, #0x1
1004646cc:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
1004646d0:      ldur    x8, [x29, #-0x70]
1004646d4:      ldp q1, q0, [x29, #-0x90]
1004646d8:      stur    q1, [sp, #0x38]
1004646dc:      stur    q0, [sp, #0x48]
1004646e0:      str x8, [sp, #0x58]
1004646e4:      ldr x1, [x20, #0x18]
1004646e8:      sub x0, x29, #0x90
1004646ec:      bl  0x100464fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1004646f0:      ldur    w8, [x29, #-0x90]
1004646f4:      cmn w8, #0x1
1004646f8:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
1004646fc:      ldur    x8, [x29, #-0x70]
100464700:      ldp q1, q0, [x29, #-0x90]
100464704:      stur    q1, [x22, #0x28]
100464708:      stur    q0, [x22, #0x38]
10046470c:      str x8, [sp, #0xf8]
100464710:      ldp w10, w8, [sp, #0x38]
100464714:      ldr w9, [sp, #0x40]
100464718:      cbz w10, 0x100464b4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ec>
10046471c:      cmp w10, #0x1
100464720:      b.ne    0x100464b5c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6fc>
100464724:      ldr w8, [sp, #0x44]
100464728:      b   0x100464b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x700>
10046472c:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape24REPARSE_MATERIALIZATIONS0s_023___RUST_STD_INTERNAL_VAL+0x8>
100464730:      add x0, x0, #0x208
100464734:      ldr x8, [x0]
100464738:      blr x8
10046473c:      ldrb    w8, [x0, #0x20]
100464740:      cbnz    w8, 0x100464e7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa1c>
100464744:      ldr x8, [x0]
100464748:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10046474c:      cmp x8, x9
100464750:      b.hs    0x100464eac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa4c>
100464754:      ldr x22, [x0, #0x18]
100464758:      stur    x22, [x29, #-0x68]
10046475c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100464760:      stp x20, x8, [x29, #-0x88]
100464764:      mov w8, #0x1                ; =1
100464768:      stur    x8, [x29, #-0x90]
10046476c:      sub x0, x29, #0x90
100464770:      bl  0x10043fbe8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100464774:      mov x24, x0
100464778:      stur    x0, [x29, #-0xc0]
10046477c:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape24REPARSE_MATERIALIZATIONS0s_023___RUST_STD_INTERNAL_VAL+0x8>
100464780:      add x0, x0, #0x178
100464784:      ldr x8, [x0]
100464788:      blr x8
10046478c:      strb    wzr, [x0]
100464790:      mov x0, x20
100464794:      bl  0x100306e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
100464798:      tbz w0, #0x0, 0x100464828 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3c8>
10046479c:      mov x0, x21
1004647a0:      bl  0x1004537ac <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
1004647a4:      mov x20, x0
1004647a8:      mov x23, x1
1004647ac:      ldr x8, [x25]
1004647b0:      cmn x8, #0x1
1004647b4:      b.eq    0x10046486c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x40c>
1004647b8:      mrs x9, TPIDRRO_EL0
1004647bc:      and x9, x9, #0xfffffffffffffff8
1004647c0:      ldr x8, [x9, x8, lsl #3]
1004647c4:      cbz x8, 0x10046486c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x40c>
1004647c8:      ldr x8, [x8, #0x19e8]
1004647cc:      cbz x8, 0x100464c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7e4>
1004647d0:      ldr x9, [x8]
1004647d4:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004647d8:      cmp x9, x10
1004647dc:      b.hs    0x100464f64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb04>
1004647e0:      add x10, x9, #0x1
1004647e4:      str x10, [x8]
1004647e8:      ldr x10, [x8, #0x18]
1004647ec:      cmp x24, x10
1004647f0:      b.hs    0x100464e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa18>
1004647f4:      ldr x10, [x8, #0x10]
1004647f8:      mov w11, #0x18              ; =24
1004647fc:      madd    x10, x24, x11, x10
100464800:      ldr x11, [x10]
100464804:      cmp x11, #0x1
100464808:      b.ne    0x100464f70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb10>
10046480c:      ldr x24, [x10, #0x8]
100464810:      str x9, [x8]
100464814:      ldr w28, [x24, #0x4]
100464818:      ldr w26, [x26]
10046481c:      cmp w26, #0x300
100464820:      b.lo    0x1004648d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x478>
100464824:      b   0x100464cd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x870>
100464828:      ldr x8, [x25]
10046482c:      cmn x8, #0x1
100464830:      b.eq    0x100464aa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x644>
100464834:      mrs x9, TPIDRRO_EL0
100464838:      and x9, x9, #0xfffffffffffffff8
10046483c:      ldr x8, [x9, x8, lsl #3]
100464840:      cbz x8, 0x100464aa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x644>
100464844:      ldr x8, [x8, #0x19e8]
100464848:      cbz x8, 0x100464c6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x80c>
10046484c:      ldr x9, [x8]
100464850:      cbnz    x9, 0x100464ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa74>
100464854:      ldr x9, [x8, #0x18]
100464858:      cmp x22, x9
10046485c:      b.hi    0x100464864 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x404>
100464860:      str x22, [x8, #0x18]
100464864:      str xzr, [x8]
100464868:      b   0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
10046486c:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape24REPARSE_MATERIALIZATIONS0s_023___RUST_STD_INTERNAL_VAL+0x8>
100464870:      add x0, x0, #0x208
100464874:      ldr x8, [x0]
100464878:      blr x8
10046487c:      ldrb    w8, [x0, #0x20]
100464880:      cbnz    w8, 0x100464ee0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa80>
100464884:      ldr x8, [x0]
100464888:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10046488c:      cmp x8, x9
100464890:      b.hs    0x100464f18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xab8>
100464894:      add x9, x8, #0x1
100464898:      str x9, [x0]
10046489c:      ldr x9, [x0, #0x18]
1004648a0:      cmp x24, x9
1004648a4:      b.hs    0x100464e78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa18>
1004648a8:      ldr x9, [x0, #0x10]
1004648ac:      mov w10, #0x18              ; =24
1004648b0:      madd    x9, x24, x10, x9
1004648b4:      ldr x10, [x9]
1004648b8:      cmp x10, #0x1
1004648bc:      b.ne    0x100464eb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa58>
1004648c0:      ldr x24, [x9, #0x8]
1004648c4:      str x8, [x0]
1004648c8:      ldr w28, [x24, #0x4]
1004648cc:      ldr w26, [x26]
1004648d0:      cmp w26, #0x300
1004648d4:      b.hs    0x100464cd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x870>
1004648d8:      ldr x8, [x25]
1004648dc:      cmn x8, #0x1
1004648e0:      b.eq    0x100464cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x860>
1004648e4:      mrs x9, TPIDRRO_EL0
1004648e8:      and x9, x9, #0xfffffffffffffff8
1004648ec:      ldr x0, [x9, x8, lsl #3]
1004648f0:      cbz x0, 0x100464cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x860>
1004648f4:      add x8, x0, x26, lsl #3
1004648f8:      ldr x0, [x8, #0x1e8]
1004648fc:      cbz x0, 0x100464cd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x870>
100464900:      str x22, [sp, #0x8]
100464904:      ldr x0, [x0]
100464908:      cbz x0, 0x100464ce8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x888>
10046490c:      ldr x8, [x0, #0x5190]
100464910:      ubfx    x9, x28, #15, #15
100464914:      ubfx    x10, x28, #5, #10
100464918:      and x11, x28, #0x1f
10046491c:      ldr x8, [x8, x9, lsl #3]
100464920:      ldr x8, [x8, x10, lsl #3]
100464924:      lsl x9, x11, #5
100464928:      ldr x26, [x8, x9]
10046492c:      stp w27, w21, [x20]
100464930:      stp wzr, wzr, [x20, #0xc]
100464934:      str w21, [x20, #0x8]
100464938:      mov w8, #0x7b               ; =123
10046493c:      mov x2, x23
100464940:      strb    w8, [x2], #0x1
100464944:      ldr x1, [x26, #0x8]
100464948:      add x21, sp, #0x10
10046494c:      add x0, sp, #0x10
100464950:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100464954:      add x8, x23, x0
100464958:      mov w27, #0x3a              ; =58
10046495c:      strb    w27, [x8, #0x1]
100464960:      add x22, x0, #0x2
100464964:      ldr x1, [x24, #0x10]
100464968:      add x28, sp, #0xb0
10046496c:      add x0, sp, #0xb0
100464970:      add x2, x23, x22
100464974:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100464978:      add x8, x0, x22
10046497c:      cmp x19, #0x1
100464980:      b.eq    0x100464a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5f4>
100464984:      mov w9, #0x2c               ; =44
100464988:      strb    w9, [x23, x8]
10046498c:      add x22, x8, #0x1
100464990:      ldr x1, [x26, #0x10]
100464994:      add x0, x21, #0x28
100464998:      add x2, x23, x22
10046499c:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1004649a0:      add x8, x0, x22
1004649a4:      strb    w27, [x23, x8]
1004649a8:      add x21, x8, #0x1
1004649ac:      ldr x1, [x24, #0x18]
1004649b0:      add x0, x28, #0x28
1004649b4:      add x2, x23, x21
1004649b8:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1004649bc:      add x8, x0, x21
1004649c0:      cmp x19, #0x2
1004649c4:      b.eq    0x100464a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5f4>
1004649c8:      mov w9, #0x2c               ; =44
1004649cc:      strb    w9, [x23, x8]
1004649d0:      add x22, x8, #0x1
1004649d4:      add x21, sp, #0x10
1004649d8:      ldr x1, [x26, #0x18]
1004649dc:      add x0, x21, #0x50
1004649e0:      add x2, x23, x22
1004649e4:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1004649e8:      add x8, x0, x22
1004649ec:      mov w28, #0x3a              ; =58
1004649f0:      strb    w28, [x23, x8]
1004649f4:      add x22, x8, #0x1
1004649f8:      add x27, sp, #0xb0
1004649fc:      ldr x1, [x24, #0x20]
100464a00:      add x0, x27, #0x50
100464a04:      add x2, x23, x22
100464a08:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100464a0c:      add x8, x0, x22
100464a10:      cmp x19, #0x3
100464a14:      b.eq    0x100464a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5f4>
100464a18:      mov w9, #0x2c               ; =44
100464a1c:      strb    w9, [x23, x8]
100464a20:      add x19, x8, #0x1
100464a24:      ldr x1, [x26, #0x20]
100464a28:      add x0, x21, #0x78
100464a2c:      add x2, x23, x19
100464a30:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100464a34:      add x8, x0, x19
100464a38:      strb    w28, [x23, x8]
100464a3c:      add x19, x8, #0x1
100464a40:      ldr x1, [x24, #0x28]
100464a44:      add x0, x27, #0x78
100464a48:      add x2, x23, x19
100464a4c:      bl  0x10046415c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100464a50:      add x8, x0, x19
100464a54:      ldr x10, [sp, #0x8]
100464a58:      mov w9, #0x7d               ; =125
100464a5c:      strb    w9, [x23, x8]
100464a60:      ldr x8, [x25]
100464a64:      cmn x8, #0x1
100464a68:      b.eq    0x100464ad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x678>
100464a6c:      mrs x9, TPIDRRO_EL0
100464a70:      and x9, x9, #0xfffffffffffffff8
100464a74:      ldr x8, [x9, x8, lsl #3]
100464a78:      cbz x8, 0x100464ad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x678>
100464a7c:      ldr x8, [x8, #0x19e8]
100464a80:      cbz x8, 0x100464ca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x840>
100464a84:      ldr x9, [x8]
100464a88:      cbnz    x9, 0x100464ed4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa74>
100464a8c:      ldr x9, [x8, #0x18]
100464a90:      cmp x10, x9
100464a94:      b.hi    0x100464a9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x63c>
100464a98:      str x10, [x8, #0x18]
100464a9c:      str xzr, [x8]
100464aa0:      b   0x100464cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x850>
100464aa4:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape24REPARSE_MATERIALIZATIONS0s_023___RUST_STD_INTERNAL_VAL+0x8>
100464aa8:      add x0, x0, #0x208
100464aac:      ldr x8, [x0]
100464ab0:      blr x8
100464ab4:      ldrb    w8, [x0, #0x20]
100464ab8:      cbnz    w8, 0x100464f24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xac4>
100464abc:      ldr x8, [x0]
100464ac0:      cbnz    x8, 0x100464fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb44>
100464ac4:      ldr x8, [x0, #0x18]
100464ac8:      cmp x22, x8
100464acc:      b.hi    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464ad0:      str x22, [x0, #0x18]
100464ad4:      b   0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464ad8:      adrp    x0, 0x10112b000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape24REPARSE_MATERIALIZATIONS0s_023___RUST_STD_INTERNAL_VAL+0x8>
100464adc:      add x0, x0, #0x208
100464ae0:      ldr x8, [x0]
100464ae4:      blr x8
100464ae8:      ldrb    w8, [x0, #0x20]
100464aec:      cbnz    w8, 0x100464f50 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf0>
100464af0:      ldr x8, [x0]
100464af4:      cbnz    x8, 0x100464fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb44>
100464af8:      ldr x8, [x0, #0x18]
100464afc:      cmp x10, x8
100464b00:      b.hi    0x100464cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x850>
100464b04:      str x10, [x0, #0x18]
100464b08:      b   0x100464cb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x850>
100464b0c:      bl  0x100cc3788 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100464b10:      add x8, x0, x24, lsl #3
100464b14:      ldr x0, [x8, #0x1e8]
100464b18:      cbnz    x0, 0x100464544 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xe4>
100464b1c:      adrp    x0, 0x1010b8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise9rejection21PROCESSING_REJECTIONS+0x790>
100464b20:      add x0, x0, #0xad0
100464b24:      bl  0x100cc2edc <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
100464b28:      ldr x0, [x0]
100464b2c:      cbnz    x0, 0x10046454c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xec>
100464b30:      bl  0x100cbdc18 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
100464b34:      b   0x10046454c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xec>
100464b38:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464b3c:      add x0, x0, #0xa80
100464b40:      bl  0x1001356ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100464b44:      mov x22, x0
100464b48:      b   0x100464758 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2f8>
100464b4c:      add w10, w8, #0x2
100464b50:      add w8, w9, #0x2
100464b54:      mov x9, x10
100464b58:      b   0x100464b60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x700>
100464b5c:      mov x9, x8
100464b60:      ldp w12, w10, [sp, #0xd8]
100464b64:      ldr w11, [sp, #0xe0]
100464b68:      cbz w12, 0x100464b7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x71c>
100464b6c:      cmp w12, #0x1
100464b70:      b.ne    0x100464b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x72c>
100464b74:      ldr w10, [sp, #0xe4]
100464b78:      b   0x100464b90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x730>
100464b7c:      add w12, w10, #0x2
100464b80:      add w10, w11, #0x2
100464b84:      mov x11, x12
100464b88:      b   0x100464b90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x730>
100464b8c:      mov x11, x10
100464b90:      adds    w9, w9, w21
100464b94:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464b98:      adds    w9, w11, w9
100464b9c:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464ba0:      cmn w9, #0x3
100464ba4:      b.hi    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464ba8:      add w8, w8, w27
100464bac:      cmp w8, w28
100464bb0:      b.ls    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464bb4:      mov x0, #0x0                ; =0
100464bb8:      adds    w8, w10, w8
100464bbc:      b.hs    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464bc0:      cmn w8, #0x3
100464bc4:      b.hi    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464bc8:      add w21, w9, #0x2
100464bcc:      add w27, w8, #0x2
100464bd0:      cmp x19, #0x2
100464bd4:      b.eq    0x10046467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x21c>
100464bd8:      ldr x1, [x24, #0x18]
100464bdc:      sub x0, x29, #0x90
100464be0:      bl  0x100465274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
100464be4:      ldur    w8, [x29, #-0x90]
100464be8:      cmn w8, #0x1
100464bec:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464bf0:      ldur    x8, [x29, #-0x70]
100464bf4:      ldp q1, q0, [x29, #-0x90]
100464bf8:      stp q1, q0, [sp, #0x60]
100464bfc:      str x8, [sp, #0x80]
100464c00:      ldr x1, [x20, #0x20]
100464c04:      sub x0, x29, #0x90
100464c08:      bl  0x100464fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
100464c0c:      ldur    w8, [x29, #-0x90]
100464c10:      cmn w8, #0x1
100464c14:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464c18:      ldur    x8, [x29, #-0x70]
100464c1c:      ldp q1, q0, [x29, #-0x90]
100464c20:      stp q1, q0, [sp, #0x100]
100464c24:      str x8, [sp, #0x120]
100464c28:      ldp w10, w8, [sp, #0x60]
100464c2c:      ldr w9, [sp, #0x68]
100464c30:      cbz w10, 0x100464cf0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x890>
100464c34:      cmp w10, #0x1
100464c38:      b.ne    0x100464d00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a0>
100464c3c:      ldr w8, [sp, #0x6c]
100464c40:      b   0x100464d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a4>
100464c44:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464c48:      add x0, x0, #0xa80
100464c4c:      sub x1, x29, #0xc0
100464c50:      bl  0x1001354d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100464c54:      mov x24, x0
100464c58:      ldr w28, [x0, #0x4]
100464c5c:      ldr w26, [x26]
100464c60:      cmp w26, #0x300
100464c64:      b.lo    0x1004648d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x478>
100464c68:      b   0x100464cd0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x870>
100464c6c:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464c70:      add x0, x0, #0xa80
100464c74:      sub x1, x29, #0x68
100464c78:      bl  0x100135a88 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100464c7c:      mov x0, #0x0                ; =0
100464c80:      add sp, sp, #0x1c0
100464c84:      ldp x29, x30, [sp, #0x50]
100464c88:      ldp x20, x19, [sp, #0x40]
100464c8c:      ldp x22, x21, [sp, #0x30]
100464c90:      ldp x24, x23, [sp, #0x20]
100464c94:      ldp x26, x25, [sp, #0x10]
100464c98:      ldp x28, x27, [sp], #0x60
100464c9c:      ret
100464ca0:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464ca4:      add x0, x0, #0xa80
100464ca8:      sub x1, x29, #0x68
100464cac:      bl  0x100135a88 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100464cb0:      mov x1, #0x7fff000000000000 ; =9223090561878065152
100464cb4:      bfxil   x1, x20, #0, #48
100464cb8:      mov w0, #0x1                ; =1
100464cbc:      b   0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464cc0:      bl  0x100cc3788 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100464cc4:      add x8, x0, x26, lsl #3
100464cc8:      ldr x0, [x8, #0x1e8]
100464ccc:      cbnz    x0, 0x100464900 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x4a0>
100464cd0:      adrp    x0, 0x1010b8000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime7promise9rejection21PROCESSING_REJECTIONS+0x790>
100464cd4:      add x0, x0, #0xad0
100464cd8:      bl  0x100cc2edc <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
100464cdc:      str x22, [sp, #0x8]
100464ce0:      ldr x0, [x0]
100464ce4:      cbnz    x0, 0x10046490c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x4ac>
100464ce8:      bl  0x100cbdc18 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
100464cec:      b   0x10046490c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x4ac>
100464cf0:      add w10, w8, #0x2
100464cf4:      add w8, w9, #0x2
100464cf8:      mov x9, x10
100464cfc:      b   0x100464d04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a4>
100464d00:      mov x9, x8
100464d04:      ldr w12, [sp, #0x100]
100464d08:      ldr w10, [sp, #0x104]
100464d0c:      ldr w11, [sp, #0x108]
100464d10:      cbz w12, 0x100464d24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8c4>
100464d14:      cmp w12, #0x1
100464d18:      b.ne    0x100464d34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8d4>
100464d1c:      ldr w10, [sp, #0x10c]
100464d20:      b   0x100464d38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8d8>
100464d24:      add w12, w10, #0x2
100464d28:      add w10, w11, #0x2
100464d2c:      mov x11, x12
100464d30:      b   0x100464d38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8d8>
100464d34:      mov x11, x10
100464d38:      adds    w9, w9, w21
100464d3c:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464d40:      adds    w9, w11, w9
100464d44:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464d48:      cmn w9, #0x3
100464d4c:      b.hi    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464d50:      adds    w8, w8, w27
100464d54:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464d58:      mov x0, #0x0                ; =0
100464d5c:      adds    w8, w10, w8
100464d60:      b.hs    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464d64:      cmn w8, #0x3
100464d68:      b.hi    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464d6c:      add w21, w9, #0x2
100464d70:      add w27, w8, #0x2
100464d74:      cmp x19, #0x3
100464d78:      b.eq    0x10046467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x21c>
100464d7c:      ldr x1, [x24, #0x20]
100464d80:      sub x0, x29, #0x90
100464d84:      bl  0x100465274 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
100464d88:      ldur    w8, [x29, #-0x90]
100464d8c:      cmn w8, #0x1
100464d90:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464d94:      ldur    x8, [x29, #-0x70]
100464d98:      ldp q1, q0, [x29, #-0x90]
100464d9c:      stur    q1, [sp, #0x88]
100464da0:      stur    q0, [x23, #0x88]
100464da4:      str x8, [sp, #0xa8]
100464da8:      ldr x1, [x20, #0x28]
100464dac:      sub x0, x29, #0x90
100464db0:      bl  0x100464fb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
100464db4:      ldur    w8, [x29, #-0x90]
100464db8:      cmn w8, #0x1
100464dbc:      b.eq    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464dc0:      ldur    x8, [x29, #-0x70]
100464dc4:      ldp q1, q0, [x29, #-0x90]
100464dc8:      stur    q1, [x22, #0x78]
100464dcc:      stur    q0, [x22, #0x88]
100464dd0:      str x8, [sp, #0x148]
100464dd4:      ldp w10, w8, [sp, #0x88]
100464dd8:      ldr w9, [sp, #0x90]
100464ddc:      cbz w10, 0x100464df0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x990>
100464de0:      cmp w10, #0x1
100464de4:      b.ne    0x100464e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a0>
100464de8:      ldr w8, [sp, #0x94]
100464dec:      b   0x100464e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a4>
100464df0:      add w10, w8, #0x2
100464df4:      add w8, w9, #0x2
100464df8:      mov x9, x10
100464dfc:      b   0x100464e04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a4>
100464e00:      mov x9, x8
100464e04:      ldr w12, [sp, #0x128]
100464e08:      ldr w10, [sp, #0x12c]
100464e0c:      ldr w11, [sp, #0x130]
100464e10:      cbz w12, 0x100464e24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9c4>
100464e14:      cmp w12, #0x1
100464e18:      b.ne    0x100464e34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9d4>
100464e1c:      ldr w10, [sp, #0x134]
100464e20:      b   0x100464e38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9d8>
100464e24:      add w12, w10, #0x2
100464e28:      add w10, w11, #0x2
100464e2c:      mov x11, x12
100464e30:      b   0x100464e38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9d8>
100464e34:      mov x11, x10
100464e38:      adds    w9, w9, w21
100464e3c:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464e40:      adds    w9, w11, w9
100464e44:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464e48:      cmn w9, #0x3
100464e4c:      b.hi    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464e50:      adds    w8, w8, w27
100464e54:      b.hs    0x100464c7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x81c>
100464e58:      mov x0, #0x0                ; =0
100464e5c:      adds    w8, w10, w8
100464e60:      b.hs    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464e64:      cmn w8, #0x3
100464e68:      b.hi    0x100464c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x820>
100464e6c:      add w21, w9, #0x2
100464e70:      add w27, w8, #0x2
100464e74:      b   0x10046467c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x21c>
100464e78:      bl  0x100cb25e8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100464e7c:      cmp w8, #0x1
100464e80:      b.ne    0x100464f58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf8>
100464e84:      adrp    x1, 0x1006c3000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x294>
100464e88:      add x1, x1, #0xad4
100464e8c:      mov x22, x0
100464e90:      bl  0x100b983dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100464e94:      mov x0, x22
100464e98:      strb    wzr, [x22, #0x20]
100464e9c:      ldr x8, [x22]
100464ea0:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100464ea4:      cmp x8, x9
100464ea8:      b.lo    0x100464754 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2f4>
100464eac:      adrp    x0, 0x101094000 <_anon.68a532d94142320e15103d7866c451bd.21>
100464eb0:      add x0, x0, #0x468
100464eb4:      bl  0x100c8a09c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100464eb8:      adrp    x0, 0x100db6000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2970>
100464ebc:      add x0, x0, #0x520
100464ec0:      mov w1, #0xb                ; =11
100464ec4:      bl  0x100cb25b0 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100464ec8:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464ecc:      add x0, x0, #0xb78
100464ed0:      bl  0x100c8a09c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100464ed4:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464ed8:      add x0, x0, #0xd28
100464edc:      bl  0x100c8a06c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100464ee0:      cmp w8, #0x2
100464ee4:      b.eq    0x100464f58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf8>
100464ee8:      mov x28, x25
100464eec:      adrp    x1, 0x1006c3000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x294>
100464ef0:      add x1, x1, #0xad4
100464ef4:      mov x25, x0
100464ef8:      bl  0x100b983dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100464efc:      mov x0, x25
100464f00:      strb    wzr, [x25, #0x20]
100464f04:      mov x25, x28
100464f08:      ldr x8, [x0]
100464f0c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100464f10:      cmp x8, x9
100464f14:      b.lo    0x100464894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x434>
100464f18:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100464f1c:      add x0, x0, #0xf70
100464f20:      bl  0x100c8a09c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100464f24:      cmp w8, #0x2
100464f28:      b.eq    0x100464f58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf8>
100464f2c:      adrp    x1, 0x1006c3000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x294>
100464f30:      add x1, x1, #0xad4
100464f34:      mov x19, x0
100464f38:      bl  0x100b983dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100464f3c:      mov x0, x19
100464f40:      strb    wzr, [x19, #0x20]
100464f44:      ldr x8, [x19]
100464f48:      cbz x8, 0x100464ac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x664>
100464f4c:      b   0x100464fa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb44>
100464f50:      cmp w8, #0x2
100464f54:      b.ne    0x100464f80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb20>
100464f58:      adrp    x0, 0x101093000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100464f5c:      add x0, x0, #0xed8
100464f60:      bl  0x100ccfd1c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100464f64:      adrp    x0, 0x1010a4000 <_anon.88ed17a1392924f08814ef64693a15d8.653+0x90>
100464f68:      add x0, x0, #0xb30
100464f6c:      bl  0x100c8a09c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100464f70:      adrp    x0, 0x100dd1000 <_anon.88ed17a1392924f08814ef64693a15d8.1611+0x478>
100464f74:      add x0, x0, #0x5be
100464f78:      mov w1, #0xb                ; =11
100464f7c:      bl  0x100cb25b0 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100464f80:      adrp    x1, 0x1006c3000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x294>
100464f84:      add x1, x1, #0xad4
100464f88:      mov x19, x0
100464f8c:      bl  0x100b983dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100464f90:      mov x0, x19
100464f94:      strb    wzr, [x19, #0x20]
100464f98:      ldr x10, [sp, #0x8]
100464f9c:      ldr x8, [x19]
100464fa0:      cbz x8, 0x100464af8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x698>
100464fa4:      adrp    x0, 0x101099000 <_anon.68a532d94142320e15103d7866c451bd.1142>
100464fa8:      add x0, x0, #0x270
100464fac:      bl  0x100c8a06c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
