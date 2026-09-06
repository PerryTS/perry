
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/empty-object-leaf-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002c72d8 <_js_json_parse>:
1002c72d8:      sub sp, sp, #0x90
1002c72dc:      stp d9, d8, [sp, #0x20]
1002c72e0:      stp x28, x27, [sp, #0x30]
1002c72e4:      stp x26, x25, [sp, #0x40]
1002c72e8:      stp x24, x23, [sp, #0x50]
1002c72ec:      stp x22, x21, [sp, #0x60]
1002c72f0:      stp x20, x19, [sp, #0x70]
1002c72f4:      stp x29, x30, [sp, #0x80]
1002c72f8:      add x29, sp, #0x80
1002c72fc:      cbz x0, 0x1002c7890 <_js_json_parse+0x5b8>
1002c7300:      ldr w1, [x0, #0x4]
1002c7304:      cbz w1, 0x1002c7890 <_js_json_parse+0x5b8>
1002c7308:      ldrb    w9, [x0, #0x14]
1002c730c:      orr w8, w9, #0x20
1002c7310:      cmp w8, #0x7b
1002c7314:      b.ne    0x1002c733c <_js_json_parse+0x64>
1002c7318:      ldp x29, x30, [sp, #0x80]
1002c731c:      ldp x20, x19, [sp, #0x70]
1002c7320:      ldp x22, x21, [sp, #0x60]
1002c7324:      ldp x24, x23, [sp, #0x50]
1002c7328:      ldp x26, x25, [sp, #0x40]
1002c732c:      ldp x28, x27, [sp, #0x30]
1002c7330:      ldp d9, d8, [sp, #0x20]
1002c7334:      add sp, sp, #0x90
1002c7338:      b   0x10028c7c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>
1002c733c:      mov x26, #0x0               ; =0
1002c7340:      sub x28, x1, #0x2
1002c7344:      mov x12, #-0x20000000000    ; =-2199023255552
1002c7348:      sub x10, x1, #0x1
1002c734c:      mov x24, #-0x15             ; =-21
1002c7350:      mov x8, #0x2600             ; =9728
1002c7354:      movk    x8, #0x1, lsl #32
1002c7358:      mov x11, #-0x10000000000    ; =-1099511627776
1002c735c:      sub x25, x1, #0x2
1002c7360:      add x21, x12, x1, lsl #40
1002c7364:      cmp w9, #0x20
1002c7368:      b.hi    0x1002c73a0 <_js_json_parse+0xc8>
1002c736c:      mov w12, w9
1002c7370:      lsr x12, x8, x12
1002c7374:      tbz w12, #0x0, 0x1002c73a0 <_js_json_parse+0xc8>
1002c7378:      cmp x10, x26
1002c737c:      b.eq    0x1002c7318 <_js_json_parse+0x40>
1002c7380:      add x9, x0, x26
1002c7384:      ldrb    w9, [x9, #0x15]
1002c7388:      sub x25, x25, #0x1
1002c738c:      add x26, x26, #0x1
1002c7390:      add x21, x21, x11
1002c7394:      sub x24, x24, #0x1
1002c7398:      cmp w9, #0x20
1002c739c:      b.ls    0x1002c736c <_js_json_parse+0x94>
1002c73a0:      add x23, x0, #0x13
1002c73a4:      add x11, x0, x26
1002c73a8:      mov x8, #0x2600             ; =9728
1002c73ac:      movk    x8, #0x1, lsl #32
1002c73b0:      mov x13, #-0x10000000000    ; =-1099511627776
1002c73b4:      mov x22, x26
1002c73b8:      ldrb    w12, [x23, x1]
1002c73bc:      cmp w12, #0x20
1002c73c0:      b.hi    0x1002c73e8 <_js_json_parse+0x110>
1002c73c4:      lsr x14, x8, x12
1002c73c8:      tbz w14, #0x0, 0x1002c73e8 <_js_json_parse+0x110>
1002c73cc:      sub x23, x23, #0x1
1002c73d0:      add x22, x22, #0x1
1002c73d4:      add x21, x21, x13
1002c73d8:      sub x25, x25, #0x1
1002c73dc:      cmp x1, x22
1002c73e0:      b.ne    0x1002c73b8 <_js_json_parse+0xe0>
1002c73e4:      b   0x1002c7318 <_js_json_parse+0x40>
1002c73e8:      sub x8, x1, x22
1002c73ec:      cmp x8, #0x4
1002c73f0:      b.eq    0x1002c745c <_js_json_parse+0x184>
1002c73f4:      cmp x8, #0x5
1002c73f8:      b.ne    0x1002c7464 <_js_json_parse+0x18c>
1002c73fc:      cmp w9, #0x22
1002c7400:      b.eq    0x1002c74e8 <_js_json_parse+0x210>
1002c7404:      cmp w9, #0x2d
1002c7408:      b.eq    0x1002c7480 <_js_json_parse+0x1a8>
1002c740c:      cmp w9, #0x66
1002c7410:      b.ne    0x1002c7474 <_js_json_parse+0x19c>
1002c7414:      add x8, x0, x26
1002c7418:      ldrb    w9, [x8, #0x15]
1002c741c:      cmp w9, #0x61
1002c7420:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7424:      ldrb    w8, [x8, #0x16]
1002c7428:      cmp w8, #0x6c
1002c742c:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7430:      add x8, x0, x26
1002c7434:      ldrb    w9, [x8, #0x17]
1002c7438:      cmp w9, #0x73
1002c743c:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7440:      ldrb    w8, [x8, #0x18]
1002c7444:      cmp w8, #0x65
1002c7448:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c744c:      mov x8, #0x2                ; =2
1002c7450:      movk    x8, #0x7ffc, lsl #48
1002c7454:      orr x19, x8, #0x1
1002c7458:      b   0x1002c74ac <_js_json_parse+0x1d4>
1002c745c:      cmp w9, #0x6d
1002c7460:      b.gt    0x1002c7548 <_js_json_parse+0x270>
1002c7464:      cmp w9, #0x22
1002c7468:      b.eq    0x1002c74e8 <_js_json_parse+0x210>
1002c746c:      cmp w9, #0x2d
1002c7470:      b.eq    0x1002c7480 <_js_json_parse+0x1a8>
1002c7474:      sub w9, w9, #0x30
1002c7478:      cmp w9, #0xa
1002c747c:      b.hs    0x1002c7318 <_js_json_parse+0x40>
1002c7480:      mov x19, x0
1002c7484:      add x0, x11, #0x14
1002c7488:      mov x20, x1
1002c748c:      mov x1, x8
1002c7490:      bl  0x100275ca4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar12parse_number>
1002c7494:      mov x8, x0
1002c7498:      mov x0, x19
1002c749c:      mov x19, x1
1002c74a0:      mov x1, x20
1002c74a4:      cmp x8, #0x1
1002c74a8:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c74ac:      bl  0x10059b850 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy35gc_collect_pending_suppressed_parse>
1002c74b0:      adrp    x0, 0x1010a0000 <_anon.870e6982689d9e7f518d787cdfe70bc0.119+0x1c8>
1002c74b4:      add x0, x0, #0x1e0
1002c74b8:      bl  0x100139700 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json12parse_scalar25clear_oversized_key_cache0bEB2P_>
1002c74bc:      cbnz    w0, 0x1002c78a0 <_js_json_parse+0x5c8>
1002c74c0:      mov x0, x19
1002c74c4:      ldp x29, x30, [sp, #0x80]
1002c74c8:      ldp x20, x19, [sp, #0x70]
1002c74cc:      ldp x22, x21, [sp, #0x60]
1002c74d0:      ldp x24, x23, [sp, #0x50]
1002c74d4:      ldp x26, x25, [sp, #0x40]
1002c74d8:      ldp x28, x27, [sp, #0x30]
1002c74dc:      ldp d9, d8, [sp, #0x20]
1002c74e0:      add sp, sp, #0x90
1002c74e4:      ret
1002c74e8:      cmp x10, x22
1002c74ec:      b.eq    0x1002c7318 <_js_json_parse+0x40>
1002c74f0:      cmp x8, #0x7
1002c74f4:      b.hi    0x1002c7318 <_js_json_parse+0x40>
1002c74f8:      cmp w12, #0x22
1002c74fc:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7500:      sub x19, x8, #0x2
1002c7504:      cmp x28, x22
1002c7508:      b.ne    0x1002c75cc <_js_json_parse+0x2f4>
1002c750c:      add x8, x0, x26
1002c7510:      add x20, x8, #0x15
1002c7514:      add x8, sp, #0x8
1002c7518:      str x0, [sp]
1002c751c:      mov x0, x20
1002c7520:      mov x27, x1
1002c7524:      mov x1, x19
1002c7528:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1002c752c:      ldp x0, x8, [sp]
1002c7530:      mov x1, x27
1002c7534:      cbnz    x8, 0x1002c7318 <_js_json_parse+0x40>
1002c7538:      cmp x28, x22
1002c753c:      b.ne    0x1002c7604 <_js_json_parse+0x32c>
1002c7540:      mov x12, #0x0               ; =0
1002c7544:      b   0x1002c7880 <_js_json_parse+0x5a8>
1002c7548:      cmp w9, #0x74
1002c754c:      b.eq    0x1002c7590 <_js_json_parse+0x2b8>
1002c7550:      cmp w9, #0x6e
1002c7554:      b.ne    0x1002c7474 <_js_json_parse+0x19c>
1002c7558:      add x8, x0, x26
1002c755c:      ldrb    w9, [x8, #0x15]
1002c7560:      cmp w9, #0x75
1002c7564:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7568:      ldrb    w8, [x8, #0x16]
1002c756c:      cmp w8, #0x6c
1002c7570:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7574:      add x8, x0, x26
1002c7578:      ldrb    w8, [x8, #0x17]
1002c757c:      cmp w8, #0x6c
1002c7580:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c7584:      mov x19, #0x2               ; =2
1002c7588:      movk    x19, #0x7ffc, lsl #48
1002c758c:      b   0x1002c74ac <_js_json_parse+0x1d4>
1002c7590:      add x8, x0, x26
1002c7594:      ldrb    w9, [x8, #0x15]
1002c7598:      cmp w9, #0x72
1002c759c:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c75a0:      ldrb    w8, [x8, #0x16]
1002c75a4:      cmp w8, #0x75
1002c75a8:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c75ac:      add x8, x0, x26
1002c75b0:      ldrb    w8, [x8, #0x17]
1002c75b4:      cmp w8, #0x65
1002c75b8:      b.ne    0x1002c7318 <_js_json_parse+0x40>
1002c75bc:      mov x8, #0x2                ; =2
1002c75c0:      movk    x8, #0x7ffc, lsl #48
1002c75c4:      add x19, x8, #0x2
1002c75c8:      b   0x1002c74ac <_js_json_parse+0x1d4>
1002c75cc:      mov x8, #0x0                ; =0
1002c75d0:      add x9, x0, x8
1002c75d4:      add x9, x9, x26
1002c75d8:      ldrb    w9, [x9, #0x15]
1002c75dc:      cmp w9, #0x20
1002c75e0:      b.lo    0x1002c7318 <_js_json_parse+0x40>
1002c75e4:      cmp w9, #0x22
1002c75e8:      b.eq    0x1002c7318 <_js_json_parse+0x40>
1002c75ec:      cmp w9, #0x5c
1002c75f0:      b.eq    0x1002c7318 <_js_json_parse+0x40>
1002c75f4:      add x8, x8, #0x1
1002c75f8:      cmp x19, x8
1002c75fc:      b.ne    0x1002c75d0 <_js_json_parse+0x2f8>
1002c7600:      b   0x1002c750c <_js_json_parse+0x234>
1002c7604:      cmp x19, #0x4
1002c7608:      b.hs    0x1002c7618 <_js_json_parse+0x340>
1002c760c:      mov x12, #0x0               ; =0
1002c7610:      mov x8, #0x0                ; =0
1002c7614:      b   0x1002c7860 <_js_json_parse+0x588>
1002c7618:      adrp    x10, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c761c:      adrp    x9, 0x100daf000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x33bda>
1002c7620:      cmp x19, #0x10
1002c7624:      b.hs    0x1002c7634 <_js_json_parse+0x35c>
1002c7628:      mov x8, #0x0                ; =0
1002c762c:      mov x12, #0x0               ; =0
1002c7630:      b   0x1002c77b8 <_js_json_parse+0x4e0>
1002c7634:      mov x12, #0x0               ; =0
1002c7638:      and x11, x19, #0xc
1002c763c:      adrp    x8, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c7640:      ldr q0, [x8, #0xc0]
1002c7644:      adrp    x8, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c7648:      ldr q1, [x8, #0xd0]
1002c764c:      and x8, x19, #0xfffffffffffffff0
1002c7650:      adrp    x13, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c7654:      ldr q2, [x13, #0xe0]
1002c7658:      adrp    x13, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c765c:      ldr q3, [x13, #0xf0]
1002c7660:      adrp    x13, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c7664:      ldr q4, [x13, #0x100]
1002c7668:      adrp    x13, 0x100db5000 <_anon.9b3cd235845189df7065721ecbc2fe65.1138+0x145>
1002c766c:      ldr q5, [x13, #0x110]
1002c7670:      mov w13, #0x10              ; =16
1002c7674:      dup.2d  v7, x13
1002c7678:      and x13, x25, #0xfffffffffffffff0
1002c767c:      add x13, x0, x13
1002c7680:      sub x20, x13, x24
1002c7684:      movi.2d v6, #0000000000000000
1002c7688:      ldr q16, [x10, #0x120]
1002c768c:      movi.2d v17, #0000000000000000
1002c7690:      movi.2d v19, #0000000000000000
1002c7694:      ldr q22, [x9, #0x3a0]
1002c7698:      movi.2d v18, #0000000000000000
1002c769c:      movi.2d v23, #0000000000000000
1002c76a0:      movi.2d v20, #0000000000000000
1002c76a4:      movi.2d v24, #0000000000000000
1002c76a8:      movi.2d v21, #0000000000000000
1002c76ac:      add x13, x0, x12
1002c76b0:      add x13, x13, x26
1002c76b4:      ldur    q25, [x13, #0x15]
1002c76b8:      ushll2.8h   v26, v25, #0x0
1002c76bc:      ushll2.4s   v27, v26, #0x0
1002c76c0:      ushll.2d    v28, v27, #0x0
1002c76c4:      ushll.4s    v26, v26, #0x0
1002c76c8:      ushll2.2d   v29, v26, #0x0
1002c76cc:      ushll.8h    v25, v25, #0x0
1002c76d0:      ushll2.4s   v30, v25, #0x0
1002c76d4:      ushll2.2d   v31, v30, #0x0
1002c76d8:      ushll2.2d   v27, v27, #0x0
1002c76dc:      ushll.2d    v26, v26, #0x0
1002c76e0:      ushll.2d    v30, v30, #0x0
1002c76e4:      ushll.4s    v25, v25, #0x0
1002c76e8:      ushll2.2d   v8, v25, #0x0
1002c76ec:      ushll.2d    v25, v25, #0x0
1002c76f0:      shl.2d  v9, v22, #0x3
1002c76f4:      ushl.2d v25, v25, v9
1002c76f8:      shl.2d  v9, v16, #0x3
1002c76fc:      ushl.2d v8, v8, v9
1002c7700:      shl.2d  v9, v5, #0x3
1002c7704:      ushl.2d v30, v30, v9
1002c7708:      shl.2d  v9, v3, #0x3
1002c770c:      ushl.2d v26, v26, v9
1002c7710:      shl.2d  v9, v0, #0x3
1002c7714:      ushl.2d v27, v27, v9
1002c7718:      shl.2d  v9, v4, #0x3
1002c771c:      ushl.2d v31, v31, v9
1002c7720:      shl.2d  v9, v2, #0x3
1002c7724:      ushl.2d v29, v29, v9
1002c7728:      shl.2d  v9, v1, #0x3
1002c772c:      ushl.2d v28, v28, v9
1002c7730:      orr.16b v24, v28, v24
1002c7734:      orr.16b v20, v29, v20
1002c7738:      orr.16b v18, v31, v18
1002c773c:      orr.16b v21, v27, v21
1002c7740:      orr.16b v23, v26, v23
1002c7744:      orr.16b v19, v30, v19
1002c7748:      orr.16b v6, v8, v6
1002c774c:      orr.16b v17, v25, v17
1002c7750:      add x12, x12, #0x10
1002c7754:      add.2d  v5, v5, v7
1002c7758:      add.2d  v16, v16, v7
1002c775c:      add.2d  v22, v22, v7
1002c7760:      add.2d  v4, v4, v7
1002c7764:      add.2d  v3, v3, v7
1002c7768:      add.2d  v2, v2, v7
1002c776c:      add.2d  v1, v1, v7
1002c7770:      add.2d  v0, v0, v7
1002c7774:      cmp x8, x12
1002c7778:      b.ne    0x1002c76ac <_js_json_parse+0x3d4>
1002c777c:      orr.16b v0, v17, v23
1002c7780:      orr.16b v1, v19, v24
1002c7784:      orr.16b v0, v0, v1
1002c7788:      orr.16b v1, v6, v20
1002c778c:      orr.16b v2, v18, v21
1002c7790:      orr.16b v1, v1, v2
1002c7794:      orr.16b v0, v0, v1
1002c7798:      mov d1, v0[1]
1002c779c:      orr.8b  v0, v0, v1
1002c77a0:      fmov    x12, d0
1002c77a4:      cmp x19, x8
1002c77a8:      b.eq    0x1002c7880 <_js_json_parse+0x5a8>
1002c77ac:      mov x1, x27
1002c77b0:      ldr x0, [sp]
1002c77b4:      cbz x11, 0x1002c7860 <_js_json_parse+0x588>
1002c77b8:      mov x11, x8
1002c77bc:      and x8, x19, #0xfffffffffffffffc
1002c77c0:      and x13, x25, #0xfffffffffffffffc
1002c77c4:      add x13, x0, x13
1002c77c8:      sub x20, x13, x24
1002c77cc:      fmov    d0, x12
1002c77d0:      movi.2d v1, #0000000000000000
1002c77d4:      dup.2d  v3, x11
1002c77d8:      ldr q2, [x10, #0x120]
1002c77dc:      orr.16b v2, v3, v2
1002c77e0:      ldr q4, [x9, #0x3a0]
1002c77e4:      orr.16b v3, v3, v4
1002c77e8:      sub x9, x11, x8
1002c77ec:      add x10, x0, x11
1002c77f0:      add x10, x10, x26
1002c77f4:      add x10, x10, #0x15
1002c77f8:      movi.2d v4, #0x000000000000ff
1002c77fc:      mov w11, #0x4               ; =4
1002c7800:      dup.2d  v5, x11
1002c7804:      ldr s6, [x10], #0x4
1002c7808:      ushll.8h    v6, v6, #0x0
1002c780c:      ushll.4s    v6, v6, #0x0
1002c7810:      ushll2.2d   v7, v6, #0x0
1002c7814:      and.16b v7, v7, v4
1002c7818:      ushll.2d    v6, v6, #0x0
1002c781c:      and.16b v6, v6, v4
1002c7820:      shl.2d  v16, v2, #0x3
1002c7824:      shl.2d  v17, v3, #0x3
1002c7828:      ushl.2d v6, v6, v17
1002c782c:      ushl.2d v7, v7, v16
1002c7830:      orr.16b v1, v7, v1
1002c7834:      orr.16b v0, v6, v0
1002c7838:      add.2d  v2, v2, v5
1002c783c:      add.2d  v3, v3, v5
1002c7840:      adds    x9, x9, #0x4
1002c7844:      b.ne    0x1002c7804 <_js_json_parse+0x52c>
1002c7848:      orr.16b v0, v0, v1
1002c784c:      mov d1, v0[1]
1002c7850:      orr.8b  v0, v0, v1
1002c7854:      fmov    x12, d0
1002c7858:      cmp x19, x8
1002c785c:      b.eq    0x1002c7880 <_js_json_parse+0x5a8>
1002c7860:      add x9, x23, x1
1002c7864:      lsl x8, x8, #3
1002c7868:      ldrb    w10, [x20], #0x1
1002c786c:      lsl x10, x10, x8
1002c7870:      orr x12, x10, x12
1002c7874:      add x8, x8, #0x8
1002c7878:      cmp x9, x20
1002c787c:      b.ne    0x1002c7868 <_js_json_parse+0x590>
1002c7880:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1002c7884:      orr x8, x21, x8
1002c7888:      orr x19, x8, x12
1002c788c:      b   0x1002c74ac <_js_json_parse+0x1d4>
1002c7890:      adrp    x0, 0x100dc2000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text9SB_KOI8_U+0x13c>
1002c7894:      add x0, x0, #0x691
1002c7898:      mov w1, #0x1c               ; =28
1002c789c:      bl  0x10028d520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1002c78a0:      bl  0x100c8e3e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar15clear_key_cache>
1002c78a4:      b   0x1002c74c0 <_js_json_parse+0x1e8>
        ...
