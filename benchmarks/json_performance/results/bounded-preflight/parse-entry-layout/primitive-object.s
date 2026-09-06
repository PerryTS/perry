
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/primitive-object-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003171fc <_js_json_parse>:
1003171fc:      sub sp, sp, #0x90
100317200:      stp d9, d8, [sp, #0x20]
100317204:      stp x28, x27, [sp, #0x30]
100317208:      stp x26, x25, [sp, #0x40]
10031720c:      stp x24, x23, [sp, #0x50]
100317210:      stp x22, x21, [sp, #0x60]
100317214:      stp x20, x19, [sp, #0x70]
100317218:      stp x29, x30, [sp, #0x80]
10031721c:      add x29, sp, #0x80
100317220:      cbz x0, 0x1003177b4 <_js_json_parse+0x5b8>
100317224:      ldr w1, [x0, #0x4]
100317228:      cbz w1, 0x1003177b4 <_js_json_parse+0x5b8>
10031722c:      ldrb    w9, [x0, #0x14]
100317230:      orr w8, w9, #0x20
100317234:      cmp w8, #0x7b
100317238:      b.ne    0x100317260 <_js_json_parse+0x64>
10031723c:      ldp x29, x30, [sp, #0x80]
100317240:      ldp x20, x19, [sp, #0x70]
100317244:      ldp x22, x21, [sp, #0x60]
100317248:      ldp x24, x23, [sp, #0x50]
10031724c:      ldp x26, x25, [sp, #0x40]
100317250:      ldp x28, x27, [sp, #0x30]
100317254:      ldp d9, d8, [sp, #0x20]
100317258:      add sp, sp, #0x90
10031725c:      b   0x1002dc708 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api10parse_slow>
100317260:      mov x26, #0x0               ; =0
100317264:      sub x28, x1, #0x2
100317268:      mov x12, #-0x20000000000    ; =-2199023255552
10031726c:      sub x10, x1, #0x1
100317270:      mov x24, #-0x15             ; =-21
100317274:      mov x8, #0x2600             ; =9728
100317278:      movk    x8, #0x1, lsl #32
10031727c:      mov x11, #-0x10000000000    ; =-1099511627776
100317280:      sub x25, x1, #0x2
100317284:      add x21, x12, x1, lsl #40
100317288:      cmp w9, #0x20
10031728c:      b.hi    0x1003172c4 <_js_json_parse+0xc8>
100317290:      mov w12, w9
100317294:      lsr x12, x8, x12
100317298:      tbz w12, #0x0, 0x1003172c4 <_js_json_parse+0xc8>
10031729c:      cmp x10, x26
1003172a0:      b.eq    0x10031723c <_js_json_parse+0x40>
1003172a4:      add x9, x0, x26
1003172a8:      ldrb    w9, [x9, #0x15]
1003172ac:      sub x25, x25, #0x1
1003172b0:      add x26, x26, #0x1
1003172b4:      add x21, x21, x11
1003172b8:      sub x24, x24, #0x1
1003172bc:      cmp w9, #0x20
1003172c0:      b.ls    0x100317290 <_js_json_parse+0x94>
1003172c4:      add x23, x0, #0x13
1003172c8:      add x11, x0, x26
1003172cc:      mov x8, #0x2600             ; =9728
1003172d0:      movk    x8, #0x1, lsl #32
1003172d4:      mov x13, #-0x10000000000    ; =-1099511627776
1003172d8:      mov x22, x26
1003172dc:      ldrb    w12, [x23, x1]
1003172e0:      cmp w12, #0x20
1003172e4:      b.hi    0x10031730c <_js_json_parse+0x110>
1003172e8:      lsr x14, x8, x12
1003172ec:      tbz w14, #0x0, 0x10031730c <_js_json_parse+0x110>
1003172f0:      sub x23, x23, #0x1
1003172f4:      add x22, x22, #0x1
1003172f8:      add x21, x21, x13
1003172fc:      sub x25, x25, #0x1
100317300:      cmp x1, x22
100317304:      b.ne    0x1003172dc <_js_json_parse+0xe0>
100317308:      b   0x10031723c <_js_json_parse+0x40>
10031730c:      sub x8, x1, x22
100317310:      cmp x8, #0x4
100317314:      b.eq    0x100317380 <_js_json_parse+0x184>
100317318:      cmp x8, #0x5
10031731c:      b.ne    0x100317388 <_js_json_parse+0x18c>
100317320:      cmp w9, #0x22
100317324:      b.eq    0x10031740c <_js_json_parse+0x210>
100317328:      cmp w9, #0x2d
10031732c:      b.eq    0x1003173a4 <_js_json_parse+0x1a8>
100317330:      cmp w9, #0x66
100317334:      b.ne    0x100317398 <_js_json_parse+0x19c>
100317338:      add x8, x0, x26
10031733c:      ldrb    w9, [x8, #0x15]
100317340:      cmp w9, #0x61
100317344:      b.ne    0x10031723c <_js_json_parse+0x40>
100317348:      ldrb    w8, [x8, #0x16]
10031734c:      cmp w8, #0x6c
100317350:      b.ne    0x10031723c <_js_json_parse+0x40>
100317354:      add x8, x0, x26
100317358:      ldrb    w9, [x8, #0x17]
10031735c:      cmp w9, #0x73
100317360:      b.ne    0x10031723c <_js_json_parse+0x40>
100317364:      ldrb    w8, [x8, #0x18]
100317368:      cmp w8, #0x65
10031736c:      b.ne    0x10031723c <_js_json_parse+0x40>
100317370:      mov x8, #0x2                ; =2
100317374:      movk    x8, #0x7ffc, lsl #48
100317378:      orr x19, x8, #0x1
10031737c:      b   0x1003173d0 <_js_json_parse+0x1d4>
100317380:      cmp w9, #0x6d
100317384:      b.gt    0x10031746c <_js_json_parse+0x270>
100317388:      cmp w9, #0x22
10031738c:      b.eq    0x10031740c <_js_json_parse+0x210>
100317390:      cmp w9, #0x2d
100317394:      b.eq    0x1003173a4 <_js_json_parse+0x1a8>
100317398:      sub w9, w9, #0x30
10031739c:      cmp w9, #0xa
1003173a0:      b.hs    0x10031723c <_js_json_parse+0x40>
1003173a4:      mov x19, x0
1003173a8:      add x0, x11, #0x14
1003173ac:      mov x20, x1
1003173b0:      mov x1, x8
1003173b4:      bl  0x1002c5c30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar12parse_number>
1003173b8:      mov x8, x0
1003173bc:      mov x0, x19
1003173c0:      mov x19, x1
1003173c4:      mov x1, x20
1003173c8:      cmp x8, #0x1
1003173cc:      b.ne    0x10031723c <_js_json_parse+0x40>
1003173d0:      bl  0x10054765c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy35gc_collect_pending_suppressed_parse>
1003173d4:      adrp    x0, 0x101098000 <_anon.9598ad47ac4696096c6725f8b9882b43.84+0x8>
1003173d8:      add x0, x0, #0x720
1003173dc:      bl  0x1001393f0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json12parse_scalar25clear_oversized_key_cache0bEB2P_>
1003173e0:      cbnz    w0, 0x1003177c4 <_js_json_parse+0x5c8>
1003173e4:      mov x0, x19
1003173e8:      ldp x29, x30, [sp, #0x80]
1003173ec:      ldp x20, x19, [sp, #0x70]
1003173f0:      ldp x22, x21, [sp, #0x60]
1003173f4:      ldp x24, x23, [sp, #0x50]
1003173f8:      ldp x26, x25, [sp, #0x40]
1003173fc:      ldp x28, x27, [sp, #0x30]
100317400:      ldp d9, d8, [sp, #0x20]
100317404:      add sp, sp, #0x90
100317408:      ret
10031740c:      cmp x10, x22
100317410:      b.eq    0x10031723c <_js_json_parse+0x40>
100317414:      cmp x8, #0x7
100317418:      b.hi    0x10031723c <_js_json_parse+0x40>
10031741c:      cmp w12, #0x22
100317420:      b.ne    0x10031723c <_js_json_parse+0x40>
100317424:      sub x19, x8, #0x2
100317428:      cmp x28, x22
10031742c:      b.ne    0x1003174f0 <_js_json_parse+0x2f4>
100317430:      add x8, x0, x26
100317434:      add x20, x8, #0x15
100317438:      add x8, sp, #0x8
10031743c:      str x0, [sp]
100317440:      mov x0, x20
100317444:      mov x27, x1
100317448:      mov x1, x19
10031744c:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
100317450:      ldp x0, x8, [sp]
100317454:      mov x1, x27
100317458:      cbnz    x8, 0x10031723c <_js_json_parse+0x40>
10031745c:      cmp x28, x22
100317460:      b.ne    0x100317528 <_js_json_parse+0x32c>
100317464:      mov x12, #0x0               ; =0
100317468:      b   0x1003177a4 <_js_json_parse+0x5a8>
10031746c:      cmp w9, #0x74
100317470:      b.eq    0x1003174b4 <_js_json_parse+0x2b8>
100317474:      cmp w9, #0x6e
100317478:      b.ne    0x100317398 <_js_json_parse+0x19c>
10031747c:      add x8, x0, x26
100317480:      ldrb    w9, [x8, #0x15]
100317484:      cmp w9, #0x75
100317488:      b.ne    0x10031723c <_js_json_parse+0x40>
10031748c:      ldrb    w8, [x8, #0x16]
100317490:      cmp w8, #0x6c
100317494:      b.ne    0x10031723c <_js_json_parse+0x40>
100317498:      add x8, x0, x26
10031749c:      ldrb    w8, [x8, #0x17]
1003174a0:      cmp w8, #0x6c
1003174a4:      b.ne    0x10031723c <_js_json_parse+0x40>
1003174a8:      mov x19, #0x2               ; =2
1003174ac:      movk    x19, #0x7ffc, lsl #48
1003174b0:      b   0x1003173d0 <_js_json_parse+0x1d4>
1003174b4:      add x8, x0, x26
1003174b8:      ldrb    w9, [x8, #0x15]
1003174bc:      cmp w9, #0x72
1003174c0:      b.ne    0x10031723c <_js_json_parse+0x40>
1003174c4:      ldrb    w8, [x8, #0x16]
1003174c8:      cmp w8, #0x75
1003174cc:      b.ne    0x10031723c <_js_json_parse+0x40>
1003174d0:      add x8, x0, x26
1003174d4:      ldrb    w8, [x8, #0x17]
1003174d8:      cmp w8, #0x65
1003174dc:      b.ne    0x10031723c <_js_json_parse+0x40>
1003174e0:      mov x8, #0x2                ; =2
1003174e4:      movk    x8, #0x7ffc, lsl #48
1003174e8:      add x19, x8, #0x2
1003174ec:      b   0x1003173d0 <_js_json_parse+0x1d4>
1003174f0:      mov x8, #0x0                ; =0
1003174f4:      add x9, x0, x8
1003174f8:      add x9, x9, x26
1003174fc:      ldrb    w9, [x9, #0x15]
100317500:      cmp w9, #0x20
100317504:      b.lo    0x10031723c <_js_json_parse+0x40>
100317508:      cmp w9, #0x22
10031750c:      b.eq    0x10031723c <_js_json_parse+0x40>
100317510:      cmp w9, #0x5c
100317514:      b.eq    0x10031723c <_js_json_parse+0x40>
100317518:      add x8, x8, #0x1
10031751c:      cmp x19, x8
100317520:      b.ne    0x1003174f4 <_js_json_parse+0x2f8>
100317524:      b   0x100317430 <_js_json_parse+0x234>
100317528:      cmp x19, #0x4
10031752c:      b.hs    0x10031753c <_js_json_parse+0x340>
100317530:      mov x12, #0x0               ; =0
100317534:      mov x8, #0x0                ; =0
100317538:      b   0x100317784 <_js_json_parse+0x588>
10031753c:      adrp    x10, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317540:      adrp    x9, 0x100daa000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x3339a>
100317544:      cmp x19, #0x10
100317548:      b.hs    0x100317558 <_js_json_parse+0x35c>
10031754c:      mov x8, #0x0                ; =0
100317550:      mov x12, #0x0               ; =0
100317554:      b   0x1003176dc <_js_json_parse+0x4e0>
100317558:      mov x12, #0x0               ; =0
10031755c:      and x11, x19, #0xc
100317560:      adrp    x8, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317564:      ldr q0, [x8, #0x980]
100317568:      adrp    x8, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
10031756c:      ldr q1, [x8, #0x990]
100317570:      and x8, x19, #0xfffffffffffffff0
100317574:      adrp    x13, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317578:      ldr q2, [x13, #0x9a0]
10031757c:      adrp    x13, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317580:      ldr q3, [x13, #0x9b0]
100317584:      adrp    x13, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317588:      ldr q4, [x13, #0x9c0]
10031758c:      adrp    x13, 0x100db0000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2ca0>
100317590:      ldr q5, [x13, #0x9d0]
100317594:      mov w13, #0x10              ; =16
100317598:      dup.2d  v7, x13
10031759c:      and x13, x25, #0xfffffffffffffff0
1003175a0:      add x13, x0, x13
1003175a4:      sub x20, x13, x24
1003175a8:      movi.2d v6, #0000000000000000
1003175ac:      ldr q16, [x10, #0x9e0]
1003175b0:      movi.2d v17, #0000000000000000
1003175b4:      movi.2d v19, #0000000000000000
1003175b8:      ldr q22, [x9, #0xbe0]
1003175bc:      movi.2d v18, #0000000000000000
1003175c0:      movi.2d v23, #0000000000000000
1003175c4:      movi.2d v20, #0000000000000000
1003175c8:      movi.2d v24, #0000000000000000
1003175cc:      movi.2d v21, #0000000000000000
1003175d0:      add x13, x0, x12
1003175d4:      add x13, x13, x26
1003175d8:      ldur    q25, [x13, #0x15]
1003175dc:      ushll2.8h   v26, v25, #0x0
1003175e0:      ushll2.4s   v27, v26, #0x0
1003175e4:      ushll.2d    v28, v27, #0x0
1003175e8:      ushll.4s    v26, v26, #0x0
1003175ec:      ushll2.2d   v29, v26, #0x0
1003175f0:      ushll.8h    v25, v25, #0x0
1003175f4:      ushll2.4s   v30, v25, #0x0
1003175f8:      ushll2.2d   v31, v30, #0x0
1003175fc:      ushll2.2d   v27, v27, #0x0
100317600:      ushll.2d    v26, v26, #0x0
100317604:      ushll.2d    v30, v30, #0x0
100317608:      ushll.4s    v25, v25, #0x0
10031760c:      ushll2.2d   v8, v25, #0x0
100317610:      ushll.2d    v25, v25, #0x0
100317614:      shl.2d  v9, v22, #0x3
100317618:      ushl.2d v25, v25, v9
10031761c:      shl.2d  v9, v16, #0x3
100317620:      ushl.2d v8, v8, v9
100317624:      shl.2d  v9, v5, #0x3
100317628:      ushl.2d v30, v30, v9
10031762c:      shl.2d  v9, v3, #0x3
100317630:      ushl.2d v26, v26, v9
100317634:      shl.2d  v9, v0, #0x3
100317638:      ushl.2d v27, v27, v9
10031763c:      shl.2d  v9, v4, #0x3
100317640:      ushl.2d v31, v31, v9
100317644:      shl.2d  v9, v2, #0x3
100317648:      ushl.2d v29, v29, v9
10031764c:      shl.2d  v9, v1, #0x3
100317650:      ushl.2d v28, v28, v9
100317654:      orr.16b v24, v28, v24
100317658:      orr.16b v20, v29, v20
10031765c:      orr.16b v18, v31, v18
100317660:      orr.16b v21, v27, v21
100317664:      orr.16b v23, v26, v23
100317668:      orr.16b v19, v30, v19
10031766c:      orr.16b v6, v8, v6
100317670:      orr.16b v17, v25, v17
100317674:      add x12, x12, #0x10
100317678:      add.2d  v5, v5, v7
10031767c:      add.2d  v16, v16, v7
100317680:      add.2d  v22, v22, v7
100317684:      add.2d  v4, v4, v7
100317688:      add.2d  v3, v3, v7
10031768c:      add.2d  v2, v2, v7
100317690:      add.2d  v1, v1, v7
100317694:      add.2d  v0, v0, v7
100317698:      cmp x8, x12
10031769c:      b.ne    0x1003175d0 <_js_json_parse+0x3d4>
1003176a0:      orr.16b v0, v17, v23
1003176a4:      orr.16b v1, v19, v24
1003176a8:      orr.16b v0, v0, v1
1003176ac:      orr.16b v1, v6, v20
1003176b0:      orr.16b v2, v18, v21
1003176b4:      orr.16b v1, v1, v2
1003176b8:      orr.16b v0, v0, v1
1003176bc:      mov d1, v0[1]
1003176c0:      orr.8b  v0, v0, v1
1003176c4:      fmov    x12, d0
1003176c8:      cmp x19, x8
1003176cc:      b.eq    0x1003177a4 <_js_json_parse+0x5a8>
1003176d0:      mov x1, x27
1003176d4:      ldr x0, [sp]
1003176d8:      cbz x11, 0x100317784 <_js_json_parse+0x588>
1003176dc:      mov x11, x8
1003176e0:      and x8, x19, #0xfffffffffffffffc
1003176e4:      and x13, x25, #0xfffffffffffffffc
1003176e8:      add x13, x0, x13
1003176ec:      sub x20, x13, x24
1003176f0:      fmov    d0, x12
1003176f4:      movi.2d v1, #0000000000000000
1003176f8:      dup.2d  v3, x11
1003176fc:      ldr q2, [x10, #0x9e0]
100317700:      orr.16b v2, v3, v2
100317704:      ldr q4, [x9, #0xbe0]
100317708:      orr.16b v3, v3, v4
10031770c:      sub x9, x11, x8
100317710:      add x10, x0, x11
100317714:      add x10, x10, x26
100317718:      add x10, x10, #0x15
10031771c:      movi.2d v4, #0x000000000000ff
100317720:      mov w11, #0x4               ; =4
100317724:      dup.2d  v5, x11
100317728:      ldr s6, [x10], #0x4
10031772c:      ushll.8h    v6, v6, #0x0
100317730:      ushll.4s    v6, v6, #0x0
100317734:      ushll2.2d   v7, v6, #0x0
100317738:      and.16b v7, v7, v4
10031773c:      ushll.2d    v6, v6, #0x0
100317740:      and.16b v6, v6, v4
100317744:      shl.2d  v16, v2, #0x3
100317748:      shl.2d  v17, v3, #0x3
10031774c:      ushl.2d v6, v6, v17
100317750:      ushl.2d v7, v7, v16
100317754:      orr.16b v1, v7, v1
100317758:      orr.16b v0, v6, v0
10031775c:      add.2d  v2, v2, v5
100317760:      add.2d  v3, v3, v5
100317764:      adds    x9, x9, #0x4
100317768:      b.ne    0x100317728 <_js_json_parse+0x52c>
10031776c:      orr.16b v0, v0, v1
100317770:      mov d1, v0[1]
100317774:      orr.8b  v0, v0, v1
100317778:      fmov    x12, d0
10031777c:      cmp x19, x8
100317780:      b.eq    0x1003177a4 <_js_json_parse+0x5a8>
100317784:      add x9, x23, x1
100317788:      lsl x8, x8, #3
10031778c:      ldrb    w10, [x20], #0x1
100317790:      lsl x10, x10, x8
100317794:      orr x12, x10, x12
100317798:      add x8, x8, #0x8
10031779c:      cmp x9, x20
1003177a0:      b.ne    0x10031778c <_js_json_parse+0x590>
1003177a4:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1003177a8:      orr x8, x21, x8
1003177ac:      orr x19, x8, x12
1003177b0:      b   0x1003173d0 <_js_json_parse+0x1d4>
1003177b4:      adrp    x0, 0x100dbb000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime4text17SB_X_MAC_CYRILLIC+0x9a>
1003177b8:      add x0, x0, #0xea1
1003177bc:      mov w1, #0x1c               ; =28
1003177c0:      bl  0x1002dd478 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api18throw_syntax_error>
1003177c4:      bl  0x100c8d030 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json12parse_scalar15clear_key_cache>
1003177c8:      b   0x1003173e4 <_js_json_parse+0x1e8>
        ...
