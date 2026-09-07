/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008c1410 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow>:
1008c1410:      sub sp, sp, #0x1a0
1008c1414:      stp x28, x27, [sp, #0x140]
1008c1418:      stp x26, x25, [sp, #0x150]
1008c141c:      stp x24, x23, [sp, #0x160]
1008c1420:      stp x22, x21, [sp, #0x170]
1008c1424:      stp x20, x19, [sp, #0x180]
1008c1428:      stp x29, x30, [sp, #0x190]
1008c142c:      add x29, sp, #0x190
1008c1430:      mov x20, x2
1008c1434:      mov x22, x1
1008c1438:      mov x19, x0
1008c143c:      add x24, sp, #0x90
1008c1440:      add x23, x1, #0x14
1008c1444:      cmp x2, #0x2
1008c1448:      b.ne    0x1008c1460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50>
1008c144c:      ldrh    w8, [x23]
1008c1450:      mov w9, #0x7d7b             ; =32123
1008c1454:      cmp w8, w9
1008c1458:      b.eq    0x1008c1494 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x84>
1008c145c:      b   0x1008c14d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1008c1460:      cmp x20, #0x3
1008c1464:      b.lo    0x1008c14d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xc4>
1008c1468:      ldrb    w8, [x23]
1008c146c:      cmp w8, #0x20
1008c1470:      b.hi    0x1008c14a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1008c1474:      mov x9, #0x2600             ; =9728
1008c1478:      movk    x9, #0x1, lsl #32
1008c147c:      lsr x9, x9, x8
1008c1480:      tbz w9, #0x0, 0x1008c14a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x90>
1008c1484:      add x0, x22, #0x14
1008c1488:      mov x1, x20
1008c148c:      bl  0x1008b8458 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty19padded_empty_object>
1008c1490:      tbz w0, #0x0, 0x1008c14cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1008c1494:      bl  0x1008b8524 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json11parse_empty21allocate_empty_object>
1008c1498:      stp xzr, x0, [x19]
1008c149c:      b   0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c14a0:      cmp w8, #0x7b
1008c14a4:      b.ne    0x1008c14cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xbc>
1008c14a8:      ldrb    w8, [x22, #0x15]
1008c14ac:      cmp w8, #0x20
1008c14b0:      b.hi    0x1008c14c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0xb4>
1008c14b4:      mov x9, #0x2600             ; =9728
1008c14b8:      movk    x9, #0x1, lsl #32
1008c14bc:      lsr x9, x9, x8
1008c14c0:      tbnz    w9, #0x0, 0x1008c1484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1008c14c4:      cmp w8, #0x7d
1008c14c8:      b.eq    0x1008c1484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x74>
1008c14cc:      cmp x20, #0x41
1008c14d0:      b.hs    0x1008c1538 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x128>
1008c14d4:      add x0, sp, #0x90
1008c14d8:      add x1, x22, #0x14
1008c14dc:      mov x2, x20
1008c14e0:      bl  0x1008b89f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object6decode>
1008c14e4:      ldr x8, [sp, #0x90]
1008c14e8:      cbz x8, 0x1008c15e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008c14ec:      ldr x8, [sp, #0x118]
1008c14f0:      str x8, [sp, #0x80]
1008c14f4:      ldur    q0, [x24, #0x48]
1008c14f8:      ldur    q1, [x24, #0x58]
1008c14fc:      stp q0, q1, [sp, #0x40]
1008c1500:      ldur    q0, [x24, #0x68]
1008c1504:      ldur    q1, [x24, #0x78]
1008c1508:      stp q0, q1, [sp, #0x60]
1008c150c:      ldur    q0, [x24, #0x8]
1008c1510:      ldur    q1, [x24, #0x18]
1008c1514:      stp q0, q1, [sp]
1008c1518:      ldur    q0, [x24, #0x28]
1008c151c:      ldur    q1, [x24, #0x38]
1008c1520:      stp q0, q1, [sp, #0x20]
1008c1524:      mov x0, sp
1008c1528:      bl  0x1008b9480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json19parse_inline_object8allocate>
1008c152c:      tbz w0, #0x0, 0x1008c15e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008c1530:      stp xzr, x1, [x19]
1008c1534:      b   0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c1538:      cmp x20, #0x3e9
1008c153c:      b.lo    0x1008c15e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008c1540:      add x0, x22, #0x14
1008c1544:      mov x1, x20
1008c1548:      mov w2, #0x3e8              ; =1000
1008c154c:      bl  0x1008b9fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c1550:      tbz w0, #0x0, 0x1008c15e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1d4>
1008c1554:      add x0, x22, #0x14
1008c1558:      mov x1, x20
1008c155c:      mov w2, #0xa120             ; =41248
1008c1560:      movk    w2, #0x7, lsl #16
1008c1564:      bl  0x1008b9fa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json6parser21nesting_depth_exceeds>
1008c1568:      tbz w0, #0x0, 0x1008c191c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x50c>
1008c156c:      adrp    x8, 0x100034000 <__RNvXs3_NtNtCsjgY6bXVaRmE_4core2io5errorNtB5_5ErrorNtNtB9_3fmt7Display3fmt+0x90>
1008c1570:      add x8, x8, #0xf80
1008c1574:      adrp    x9, 0x100e13000 <_anon.0c78480e1ec3114c482e9770ddf18575.1396+0x64>
1008c1578:      add x9, x9, #0x790
1008c157c:      stp x9, x8, [sp]
1008c1580:      adrp    x0, 0x100efc000 <_anon.0c78480e1ec3114c482e9770ddf18575.477+0x2e>
1008c1584:      add x0, x0, #0x77e
1008c1588:      add x8, sp, #0x90
1008c158c:      mov x1, sp
1008c1590:      bl  0x100023808 <__RNvNvNtCsctvjasLqLe9_5alloc3fmt6format12format_inner>
1008c1594:      ldr x20, [sp, #0x98]
1008c1598:      ldr w1, [sp, #0xa0]
1008c159c:      mov x0, x20
1008c15a0:      mov x2, x1
1008c15a4:      bl  0x1009e4440 <_js_string_from_bytes_with_capacity>
1008c15a8:      mov x3, x0
1008c15ac:      adrp    x1, 0x100e0c000 <_anon.a237fa49f331f28fb58ad898b36936d2.2333+0x209>
1008c15b0:      add x1, x1, #0xd38
1008c15b4:      mov w21, #0x1               ; =1
1008c15b8:      mov w0, #0x2                ; =2
1008c15bc:      mov w2, #0xa                ; =10
1008c15c0:      mov w4, #0x1                ; =1
1008c15c4:      bl  0x1008a7b24 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008c15c8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008c15cc:      bfxil   x8, x0, #0, #48
1008c15d0:      stp x21, x8, [x19]
1008c15d4:      ldr x8, [sp, #0x90]
1008c15d8:      cbz x8, 0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c15dc:      mov x0, x20
1008c15e0:      b   0x1008c1870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x460>
1008c15e4:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
1008c15e8:      add x0, x0, #0x2a0
1008c15ec:      ldr x8, [x0]
1008c15f0:      blr x8
1008c15f4:      mov x21, x0
1008c15f8:      ldrb    w8, [x0, #0x20]
1008c15fc:      cbnz    w8, 0x1008c19e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d8>
1008c1600:      ldr x8, [x21]
1008c1604:      cbnz    x8, 0x1008c1a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1008c1608:      mov x23, #0x7fff000000000000 ; =9223090561878065152
1008c160c:      bfxil   x23, x22, #0, #48
1008c1610:      mov x8, #-0x1               ; =-1
1008c1614:      str x8, [x21]
1008c1618:      mov x22, x21
1008c161c:      ldr x8, [x22, #0x8]!
1008c1620:      ldr x25, [x21, #0x18]
1008c1624:      cmp x25, x8
1008c1628:      b.ne    0x1008c1634 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x224>
1008c162c:      mov x0, x22
1008c1630:      bl  0x100cad9e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c1634:      ldr x8, [x21, #0x10]
1008c1638:      str x23, [x8, x25, lsl #3]
1008c163c:      add x8, x25, #0x1
1008c1640:      str x8, [x21, #0x18]
1008c1644:      ldr x8, [x21]
1008c1648:      add x8, x8, #0x1
1008c164c:      str x8, [x21]
1008c1650:      mov x0, #0x0                ; =0
1008c1654:      bl  0x1002daea8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy43GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1J_6option6OptionQIB2o_INtNtB1J_4cell4CellbEEEEE9call_onceBc_>
1008c1658:      ldrb    w8, [x0]
1008c165c:      strb    wzr, [x0]
1008c1660:      cbz w8, 0x1008c1698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x288>
1008c1664:      mov x23, x0
1008c1668:      mov x0, #0x0                ; =0
1008c166c:      bl  0x1002daec8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c1670:      ldrb    w8, [x0]
1008c1674:      tst w8, #0x3
1008c1678:      b.ne    0x1008c1690 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x280>
1008c167c:      adrp    x8, 0x10117c000 <_out_buf+0x3e08>
1008c1680:      add x8, x8, #0x440
1008c1684:      ldapr   w8, [x8]
1008c1688:      cmp w8, #0x0
1008c168c:      b.le    0x1008c1894 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x484>
1008c1690:      mov w8, #0x1                ; =1
1008c1694:      strb    w8, [x23]
1008c1698:      bl  0x1002a6240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c169c:      bl  0x1002a5c48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008c16a0:      ldrb    w8, [x21, #0x20]
1008c16a4:      cbnz    w8, 0x1008c18e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4d4>
1008c16a8:      ldr x8, [x21]
1008c16ac:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c16b0:      cmp x8, x9
1008c16b4:      b.hs    0x1008c1910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x500>
1008c16b8:      add x9, x8, #0x1
1008c16bc:      str x9, [x21]
1008c16c0:      ldr x10, [x21, #0x18]
1008c16c4:      mov w9, #0x1                ; =1
1008c16c8:      cmp x25, x10
1008c16cc:      b.hs    0x1008c16e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d0>
1008c16d0:      ldr x10, [x21, #0x10]
1008c16d4:      ldr x10, [x10, x25, lsl #3]
1008c16d8:      and x10, x10, #0xffffffffffff
1008c16dc:      b   0x1008c16e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2d4>
1008c16e0:      mov w10, #0x1               ; =1
1008c16e4:      str x8, [x21]
1008c16e8:      add x8, x10, #0x14
1008c16ec:      movi.2d v0, #0000000000000000
1008c16f0:      stur    q0, [x24, #0x78]
1008c16f4:      stur    q0, [x24, #0x68]
1008c16f8:      stur    q0, [x24, #0x58]
1008c16fc:      stur    q0, [x24, #0x48]
1008c1700:      strb    w9, [sp, #0x120]
1008c1704:      mov x9, #-0x1               ; =-1
1008c1708:      stp x8, x20, [sp, #0xb8]
1008c170c:      str x9, [sp, #0x90]
1008c1710:      stp xzr, xzr, [sp, #0xc8]
1008c1714:      str xzr, [sp, #0x118]
1008c1718:      add x0, sp, #0x90
1008c171c:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008c1720:      mov x20, x0
1008c1724:      ldp x8, x9, [sp, #0xc0]
1008c1728:      cmp x9, x8
1008c172c:      b.hs    0x1008c1760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008c1730:      ldr x10, [sp, #0xb8]
1008c1734:      mov x11, #0x2600            ; =9728
1008c1738:      movk    x11, #0x1, lsl #32
1008c173c:      ldrb    w12, [x10, x9]
1008c1740:      cmp w12, #0x20
1008c1744:      b.hi    0x1008c1760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008c1748:      lsr x12, x11, x12
1008c174c:      tbz w12, #0x0, 0x1008c1760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x350>
1008c1750:      add x9, x9, #0x1
1008c1754:      cmp x8, x9
1008c1758:      b.ne    0x1008c173c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x32c>
1008c175c:      mov x9, x8
1008c1760:      ldrb    w23, [sp, #0x120]
1008c1764:      cmp x9, x8
1008c1768:      cset    w24, eq
1008c176c:      ldrb    w8, [x21, #0x20]
1008c1770:      cbnz    w8, 0x1008c1a10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x600>
1008c1774:      ldr x8, [x21]
1008c1778:      cbnz    x8, 0x1008c1a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1008c177c:      mov x8, #-0x1               ; =-1
1008c1780:      str x8, [x21]
1008c1784:      ldr x26, [x21, #0x18]
1008c1788:      ldr x8, [x21, #0x8]
1008c178c:      cmp x26, x8
1008c1790:      b.ne    0x1008c179c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x38c>
1008c1794:      mov x0, x22
1008c1798:      bl  0x100cad9e0 <__RNvMs4_NtCsctvjasLqLe9_5alloc7raw_vecINtB5_6RawVecxE8grow_oneCs5gMwpk3Cs4e_13perry_runtime>
1008c179c:      ldr x8, [x21, #0x10]
1008c17a0:      str x20, [x8, x26, lsl #3]
1008c17a4:      add x8, x26, #0x1
1008c17a8:      str x8, [x21, #0x18]
1008c17ac:      ldr x8, [x21]
1008c17b0:      add x8, x8, #0x1
1008c17b4:      str x8, [x21]
1008c17b8:      mov x0, #0x0                ; =0
1008c17bc:      bl  0x1002daec8 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy8GC_FLAGS0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB19_6option6OptionQIB1O_INtNtB19_4cell4CellhEEEEE9call_onceBc_>
1008c17c0:      ldrb    w8, [x0]
1008c17c4:      and w8, w8, #0xfffffffd
1008c17c8:      strb    w8, [x0]
1008c17cc:      bl  0x1002a6d28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy22gc_bump_malloc_trigger>
1008c17d0:      bl  0x1002abac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy49gc_schedule_parse_boundary_collection_if_pressure>
1008c17d4:      ldrb    w8, [x21, #0x20]
1008c17d8:      cbnz    w8, 0x1008c1a40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x630>
1008c17dc:      ldr x8, [x21]
1008c17e0:      cbnz    x8, 0x1008c1a70 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x660>
1008c17e4:      and w22, w24, w23
1008c17e8:      ldr x8, [x21, #0x18]
1008c17ec:      cmp x25, x8
1008c17f0:      b.hi    0x1008c17f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3e8>
1008c17f4:      str x25, [x21, #0x18]
1008c17f8:      adrp    x0, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c17fc:      add x0, x0, #0xfd8
1008c1800:      bl  0x100139dcc <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api17parse_result_slows_0uEB2P_>
1008c1804:      tbz w22, #0x0, 0x1008c181c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x40c>
1008c1808:      stp xzr, x20, [x19]
1008c180c:      ldr x8, [sp, #0x90]
1008c1810:      cmn x8, #0x1
1008c1814:      b.ne    0x1008c1868 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x458>
1008c1818:      b   0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c181c:      adrp    x0, 0x100e10000 <_anon.0c78480e1ec3114c482e9770ddf18575.278+0x324a>
1008c1820:      add x0, x0, #0x626
1008c1824:      mov w1, #0x21               ; =33
1008c1828:      mov w2, #0x21               ; =33
1008c182c:      bl  0x1009e4440 <_js_string_from_bytes_with_capacity>
1008c1830:      mov x3, x0
1008c1834:      adrp    x1, 0x100e0c000 <_anon.a237fa49f331f28fb58ad898b36936d2.2333+0x209>
1008c1838:      add x1, x1, #0xd50
1008c183c:      mov w20, #0x1               ; =1
1008c1840:      mov w0, #0x4                ; =4
1008c1844:      mov w2, #0xb                ; =11
1008c1848:      mov w4, #0x1                ; =1
1008c184c:      bl  0x1008a7b24 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008c1850:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008c1854:      bfxil   x8, x0, #0, #48
1008c1858:      stp x20, x8, [x19]
1008c185c:      ldr x8, [sp, #0x90]
1008c1860:      cmn x8, #0x1
1008c1864:      b.eq    0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c1868:      cbz x8, 0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c186c:      ldr x0, [sp, #0x98]
1008c1870:      bl  0x100ce2ac0 <_mi_free>
1008c1874:      ldp x29, x30, [sp, #0x190]
1008c1878:      ldp x20, x19, [sp, #0x180]
1008c187c:      ldp x22, x21, [sp, #0x170]
1008c1880:      ldp x24, x23, [sp, #0x160]
1008c1884:      ldp x26, x25, [sp, #0x150]
1008c1888:      ldp x28, x27, [sp, #0x140]
1008c188c:      add sp, sp, #0x1a0
1008c1890:      ret
1008c1894:      mov x0, #0x0                ; =0
1008c1898:      bl  0x1002db010 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5block17ARENA_TOTAL_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1l_6option6OptionQIB20_INtNtB1l_4cell4CelljEEEEE9call_onceBc_>
1008c189c:      ldr x23, [x0]
1008c18a0:      mov x0, #0x0                ; =0
1008c18a4:      bl  0x1002dad68 <__RNvYNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy21GC_NEXT_TRIGGER_BYTES0s_0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceTINtNtB1n_6option6OptionQIB22_INtNtB1n_4cell4CelljEEEEE9call_onceBc_>
1008c18a8:      ldr x8, [x0]
1008c18ac:      cmp x8, x23
1008c18b0:      b.ls    0x1008c18d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x4c0>
1008c18b4:      str x23, [x0]
1008c18b8:      adrp    x0, 0x101135000 <__MergedGlobals+0xc0>
1008c18bc:      add x0, x0, #0xf90
1008c18c0:      ldr x8, [x0]
1008c18c4:      blr x8
1008c18c8:      mov w8, #0x1                ; =1
1008c18cc:      strb    w8, [x0]
1008c18d0:      bl  0x1002a6240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c18d4:      bl  0x1002a6240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy16gc_check_trigger>
1008c18d8:      bl  0x1002a5c48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy11gc_suppress>
1008c18dc:      ldrb    w8, [x21, #0x20]
1008c18e0:      cbz w8, 0x1008c16a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x298>
1008c18e4:      cmp w8, #0x2
1008c18e8:      b.eq    0x1008c1a48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1008c18ec:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c18f0:      add x1, x1, #0xeec
1008c18f4:      mov x0, x21
1008c18f8:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c18fc:      strb    wzr, [x21, #0x20]
1008c1900:      ldr x8, [x21]
1008c1904:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008c1908:      cmp x8, x9
1008c190c:      b.lo    0x1008c16b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x2a8>
1008c1910:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c1914:      add x0, x0, #0xdc8
1008c1918:      bl  0x100c99c5c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008c191c:      stur    x20, [x29, #-0x68]
1008c1920:      mov x8, #0x7fff000000000000 ; =9223090561878065152
1008c1924:      bfxil   x8, x22, #0, #48
1008c1928:      str x8, [sp, #0x90]
1008c192c:      adrp    x21, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c1930:      add x21, x21, #0xfd0
1008c1934:      add x1, sp, #0x90
1008c1938:      mov x0, x21
1008c193c:      bl  0x100137610 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json15parse_root_push0jEB2h_>
1008c1940:      mov x22, x0
1008c1944:      stp x0, x23, [x29, #-0x60]
1008c1948:      str x20, [sp]
1008c194c:      sub x8, x29, #0x58
1008c1950:      mov x9, sp
1008c1954:      stp x8, x9, [sp, #0x90]
1008c1958:      sub x8, x29, #0x60
1008c195c:      sub x9, x29, #0x68
1008c1960:      stp x8, x9, [sp, #0xa0]
1008c1964:      adrp    x0, 0x1010ce000 <_anon.a237fa49f331f28fb58ad898b36936d2.2234+0xf0>
1008c1968:      add x0, x0, #0x7c0
1008c196c:      add x1, sp, #0x90
1008c1970:      bl  0x10012c518 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell4CellINtNtBZ_6option6OptionNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape11TapeScratchEEE4withNCINvB1Q_23with_built_tape_mut_rawIB1t_NtNtNtB1S_5value7jsvalue7JSValueENCINvB1Q_19with_built_tape_rawB3o_NCNvNtNtB1S_4json9parse_api24try_parse_deep_iterative0E0E0IB1t_B3o_EEB1S_>
1008c1974:      mov x23, x0
1008c1978:      mov x20, x1
1008c197c:      str x22, [sp, #0x90]
1008c1980:      add x1, sp, #0x90
1008c1984:      mov x0, x21
1008c1988:      bl  0x1001376a8 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecdEEE4withNCNvNtCs5gMwpk3Cs4e_13perry_runtime4json18parse_root_restore0uEB2h_>
1008c198c:      adrp    x0, 0x1010cf000 <_anon.0c78480e1ec3114c482e9770ddf18575.129+0x90>
1008c1990:      add x0, x0, #0xfd8
1008c1994:      bl  0x100139f34 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBa_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs5gMwpk3Cs4e_13perry_runtime6string12StringHeaderEEE4withNCNvNtNtB2P_4json9parse_api24try_parse_deep_iteratives_0uEB2P_>
1008c1998:      tbnz    w23, #0x0, 0x1008c19dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5cc>
1008c199c:      adrp    x0, 0x100e0c000 <_anon.a237fa49f331f28fb58ad898b36936d2.2333+0x209>
1008c19a0:      add x0, x0, #0xd01
1008c19a4:      mov w1, #0x29               ; =41
1008c19a8:      mov w2, #0x29               ; =41
1008c19ac:      bl  0x1009e4440 <_js_string_from_bytes_with_capacity>
1008c19b0:      mov x3, x0
1008c19b4:      adrp    x1, 0x100e0c000 <_anon.a237fa49f331f28fb58ad898b36936d2.2333+0x209>
1008c19b8:      add x1, x1, #0xd50
1008c19bc:      mov w21, #0x1               ; =1
1008c19c0:      mov w0, #0x4                ; =4
1008c19c4:      mov w2, #0xb                ; =11
1008c19c8:      mov w4, #0x1                ; =1
1008c19cc:      bl  0x1008a7b24 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5error11alloc_error>
1008c19d0:      mov x20, #0x7ffd000000000000 ; =9222527611924643840
1008c19d4:      bfxil   x20, x0, #0, #48
1008c19d8:      b   0x1008c19e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x5d0>
1008c19dc:      mov x21, #0x0               ; =0
1008c19e0:      stp x21, x20, [x19]
1008c19e4:      b   0x1008c1874 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x464>
1008c19e8:      cmp w8, #0x1
1008c19ec:      b.ne    0x1008c1a48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1008c19f0:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c19f4:      add x1, x1, #0xeec
1008c19f8:      mov x0, x21
1008c19fc:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c1a00:      strb    wzr, [x21, #0x20]
1008c1a04:      ldr x8, [x21]
1008c1a08:      cbz x8, 0x1008c1608 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x1f8>
1008c1a0c:      b   0x1008c1a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x624>
1008c1a10:      cmp w8, #0x2
1008c1a14:      b.eq    0x1008c1a48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x638>
1008c1a18:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1a1c:      add x1, x1, #0xeec
1008c1a20:      mov x0, x21
1008c1a24:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c1a28:      strb    wzr, [x21, #0x20]
1008c1a2c:      ldr x8, [x21]
1008c1a30:      cbz x8, 0x1008c177c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x36c>
1008c1a34:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c1a38:      add x0, x0, #0xdf8
1008c1a3c:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008c1a40:      cmp w8, #0x2
1008c1a44:      b.ne    0x1008c1a54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x644>
1008c1a48:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008c1a4c:      add x0, x0, #0xed8
1008c1a50:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008c1a54:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
1008c1a58:      add x1, x1, #0xeec
1008c1a5c:      mov x0, x21
1008c1a60:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008c1a64:      strb    wzr, [x21, #0x20]
1008c1a68:      ldr x8, [x21]
1008c1a6c:      cbz x8, 0x1008c17e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json9parse_api17parse_result_slow+0x3d4>
1008c1a70:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
1008c1a74:      add x0, x0, #0xe58
1008c1a78:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
