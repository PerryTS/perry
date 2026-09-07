/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c1520 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode>:
1004c1520:      sub sp, sp, #0x1c0
1004c1524:      stp x28, x27, [sp, #0x160]
1004c1528:      stp x26, x25, [sp, #0x170]
1004c152c:      stp x24, x23, [sp, #0x180]
1004c1530:      stp x22, x21, [sp, #0x190]
1004c1534:      stp x20, x19, [sp, #0x1a0]
1004c1538:      stp x29, x30, [sp, #0x1b0]
1004c153c:      add x29, sp, #0x1b0
1004c1540:      mov x19, x2
1004c1544:      mov x8, #0x0                ; =0
1004c1548:      add x9, sp, #0x48
1004c154c:      add x22, x9, #0x10
1004c1550:      add x23, x9, #0x20
1004c1554:      add x24, x9, #0x30
1004c1558:      add x25, x9, #0x40
1004c155c:      add x26, x9, #0x50
1004c1560:      add x27, x9, #0x60
1004c1564:      add x28, x9, #0x70
1004c1568:      mov x9, #0x2600             ; =9728
1004c156c:      movk    x9, #0x1, lsl #32
1004c1570:      ldrb    w10, [x1, x8]
1004c1574:      cmp w10, #0x20
1004c1578:      b.hi    0x1004c1594 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c157c:      lsr x11, x9, x10
1004c1580:      tbz w11, #0x0, 0x1004c1594 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c1584:      add x8, x8, #0x1
1004c1588:      cmp x19, x8
1004c158c:      b.ne    0x1004c1570 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x50>
1004c1590:      b   0x1004c1b98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c1594:      cmp w10, #0x7b
1004c1598:      b.ne    0x1004c1b98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c159c:      stp x0, x1, [sp, #0x30]
1004c15a0:      add x20, x8, #0x1
1004c15a4:      str x20, [sp, #0x40]
1004c15a8:      adrp    x8, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x61e8>
1004c15ac:      add x8, x8, #0x3b0
1004c15b0:      add x0, sp, #0x48
1004c15b4:      mov x1, x8
1004c15b8:      mov w2, #0x80               ; =128
1004c15bc:      bl  0x100af8850 <_writev+0x100af8850>
1004c15c0:      ldr x0, [sp, #0x38]
1004c15c4:      mov x12, #0x0               ; =0
1004c15c8:      str xzr, [sp, #0xc8]
1004c15cc:      mov x21, #0x2600            ; =9728
1004c15d0:      movk    x21, #0x1, lsl #32
1004c15d4:      cmp x20, x19
1004c15d8:      b.hs    0x1004c1600 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c15dc:      ldrb    w8, [x0, x20]
1004c15e0:      cmp w8, #0x20
1004c15e4:      b.hi    0x1004c1600 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c15e8:      lsr x8, x21, x8
1004c15ec:      tbz w8, #0x0, 0x1004c1600 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c15f0:      add x20, x20, #0x1
1004c15f4:      str x20, [sp, #0x40]
1004c15f8:      cmp x20, x19
1004c15fc:      b.lo    0x1004c15dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xbc>
1004c1600:      str x12, [sp, #0x28]
1004c1604:      add x2, sp, #0x40
1004c1608:      mov x1, x19
1004c160c:      bl  0x1004c126c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c1610:      cmp x0, #0x1
1004c1614:      b.ne    0x1004c1bbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x69c>
1004c1618:      ldp x0, x8, [sp, #0x38]
1004c161c:      cmp x8, x19
1004c1620:      ldr x11, [sp, #0x30]
1004c1624:      b.hs    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1628:      ldrb    w9, [x0, x8]
1004c162c:      cmp w9, #0x3a
1004c1630:      b.hi    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1634:      lsr x10, x21, x9
1004c1638:      tbz w10, #0x0, 0x1004c164c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x12c>
1004c163c:      add x8, x8, #0x1
1004c1640:      cmp x19, x8
1004c1644:      b.ne    0x1004c1628 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x108>
1004c1648:      b   0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c164c:      cmp x9, #0x3a
1004c1650:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1654:      str x22, [sp, #0x20]
1004c1658:      add x8, x8, #0x1
1004c165c:      cmp x8, x19
1004c1660:      b.hs    0x1004c16b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c1664:      ldrb    w9, [x0, x8]
1004c1668:      cmp w9, #0x20
1004c166c:      b.hi    0x1004c1690 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1670:      lsr x10, x21, x9
1004c1674:      tbz w10, #0x0, 0x1004c1690 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1678:      add x8, x8, #0x1
1004c167c:      cmp x19, x8
1004c1680:      b.ne    0x1004c1664 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x144>
1004c1684:      mov x8, x19
1004c1688:      mov x9, x19
1004c168c:      b   0x1004c16f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1690:      str x8, [sp, #0x40]
1004c1694:      cmp w9, #0x22
1004c1698:      b.ne    0x1004c16b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c169c:      add x2, sp, #0x40
1004c16a0:      mov x20, x1
1004c16a4:      mov x1, x19
1004c16a8:      bl  0x1004c126c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c16ac:      b   0x1004c1854 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x334>
1004c16b0:      mov x9, x8
1004c16b4:      cmp x8, x19
1004c16b8:      b.hs    0x1004c16f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d4>
1004c16bc:      ldrb    w10, [x0, x9]
1004c16c0:      cmp w10, #0x2c
1004c16c4:      b.hi    0x1004c16d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1b8>
1004c16c8:      mov x12, #0x2600            ; =9728
1004c16cc:      movk    x12, #0x1001, lsl #32
1004c16d0:      lsr x12, x12, x10
1004c16d4:      tbnz    w12, #0x0, 0x1004c16f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c16d8:      cmp w10, #0x7d
1004c16dc:      b.eq    0x1004c16f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c16e0:      add x9, x9, #0x1
1004c16e4:      cmp x19, x9
1004c16e8:      b.ne    0x1004c16bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x19c>
1004c16ec:      mov x9, x19
1004c16f0:      b   0x1004c16f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c16f4:      mov x9, x8
1004c16f8:      str x9, [sp, #0x40]
1004c16fc:      cmp x9, x8
1004c1700:      b.lo    0x1004c1cd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c1704:      cmp x9, x19
1004c1708:      b.hi    0x1004c1cd4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c170c:      subs    x12, x9, x8
1004c1710:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1714:      add x20, x0, x8
1004c1718:      ldrb    w13, [x20]
1004c171c:      orr w10, w13, #0x20
1004c1720:      cmp w10, #0x7b
1004c1724:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1728:      mov x22, #0x0               ; =0
1004c172c:      sub x10, x12, #0x1
1004c1730:      sub x12, x12, #0x2
1004c1734:      cmp w13, #0x20
1004c1738:      b.hi    0x1004c176c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c173c:      mov w14, w13
1004c1740:      lsr x14, x21, x14
1004c1744:      tbz w14, #0x0, 0x1004c176c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c1748:      cmp x10, x22
1004c174c:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1750:      add x13, x20, x22
1004c1754:      ldrb    w13, [x13, #0x1]
1004c1758:      add x22, x22, #0x1
1004c175c:      add x8, x8, #0x1
1004c1760:      sub x12, x12, #0x1
1004c1764:      cmp w13, #0x20
1004c1768:      b.ls    0x1004c173c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x21c>
1004c176c:      sub x10, x0, #0x1
1004c1770:      mov x14, x8
1004c1774:      ldrb    w15, [x10, x9]
1004c1778:      cmp w15, #0x20
1004c177c:      b.hi    0x1004c17a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c1780:      lsr x16, x21, x15
1004c1784:      tbz w16, #0x0, 0x1004c17a0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c1788:      sub x10, x10, #0x1
1004c178c:      add x14, x14, #0x1
1004c1790:      sub x12, x12, #0x1
1004c1794:      cmp x9, x14
1004c1798:      b.ne    0x1004c1774 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x254>
1004c179c:      b   0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c17a0:      sub x10, x9, x14
1004c17a4:      cmp x10, #0x4
1004c17a8:      b.eq    0x1004c1820 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x300>
1004c17ac:      cmp x10, #0x5
1004c17b0:      b.ne    0x1004c1828 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x308>
1004c17b4:      cmp w13, #0x22
1004c17b8:      b.eq    0x1004c1998 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c17bc:      cmp w13, #0x2d
1004c17c0:      b.eq    0x1004c1844 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c17c4:      cmp w13, #0x66
1004c17c8:      b.ne    0x1004c1838 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c17cc:      add x8, x20, x22
1004c17d0:      ldrb    w9, [x8, #0x1]
1004c17d4:      cmp w9, #0x61
1004c17d8:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c17dc:      ldrb    w8, [x8, #0x2]
1004c17e0:      cmp w8, #0x6c
1004c17e4:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c17e8:      add x8, x20, x22
1004c17ec:      ldrb    w9, [x8, #0x3]
1004c17f0:      cmp w9, #0x73
1004c17f4:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c17f8:      ldrb    w8, [x8, #0x4]
1004c17fc:      cmp w8, #0x65
1004c1800:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1804:      mov x8, #0x2                ; =2
1004c1808:      movk    x8, #0x7ffc, lsl #48
1004c180c:      orr x8, x8, #0x1
1004c1810:      ldp x22, x12, [sp, #0x20]
1004c1814:      cmp x12, #0x9
1004c1818:      b.lo    0x1004c1874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c181c:      b   0x1004c1b80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1820:      cmp w13, #0x6d
1004c1824:      b.gt    0x1004c1a48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x528>
1004c1828:      cmp w13, #0x22
1004c182c:      b.eq    0x1004c1998 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c1830:      cmp w13, #0x2d
1004c1834:      b.eq    0x1004c1844 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c1838:      sub w8, w13, #0x30
1004c183c:      cmp w8, #0xa
1004c1840:      b.hs    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1844:      add x0, x20, x22
1004c1848:      mov x20, x1
1004c184c:      mov x1, x10
1004c1850:      bl  0x1004bc148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004c1854:      mov x9, x0
1004c1858:      ldp x11, x0, [sp, #0x30]
1004c185c:      tbz w9, #0x0, 0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1860:      mov x8, x1
1004c1864:      mov x1, x20
1004c1868:      ldp x22, x12, [sp, #0x20]
1004c186c:      cmp x12, #0x9
1004c1870:      b.hs    0x1004c1b80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1874:      cbz x12, 0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1878:      ldr x9, [sp, #0x48]
1004c187c:      cmp x9, x1
1004c1880:      b.ne    0x1004c1890 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x370>
1004c1884:      add x9, sp, #0x48
1004c1888:      str x8, [x9, #0x8]
1004c188c:      b   0x1004c18b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x390>
1004c1890:      cmp x12, #0x1
1004c1894:      b.ne    0x1004c18ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3cc>
1004c1898:      add x9, sp, #0x48
1004c189c:      add x9, x9, x12, lsl #4
1004c18a0:      stp x1, x8, [x9]
1004c18a4:      ldr x8, [sp, #0xc8]
1004c18a8:      add x12, x8, #0x1
1004c18ac:      str x12, [sp, #0xc8]
1004c18b0:      ldr x20, [sp, #0x40]
1004c18b4:      cmp x20, x19
1004c18b8:      b.hs    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c18bc:      ldrb    w8, [x0, x20]
1004c18c0:      cmp w8, #0x2c
1004c18c4:      b.hi    0x1004c1bc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c18c8:      lsr x9, x21, x8
1004c18cc:      tbz w9, #0x0, 0x1004c18e0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3c0>
1004c18d0:      add x20, x20, #0x1
1004c18d4:      cmp x19, x20
1004c18d8:      b.ne    0x1004c18bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x39c>
1004c18dc:      b   0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c18e0:      cmp x8, #0x2c
1004c18e4:      b.eq    0x1004c15f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xd0>
1004c18e8:      b   0x1004c1bc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c18ec:      ldr x10, [sp, #0x58]
1004c18f0:      mov x9, x22
1004c18f4:      cmp x10, x1
1004c18f8:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c18fc:      cmp x12, #0x2
1004c1900:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1904:      ldr x10, [sp, #0x68]
1004c1908:      mov x9, x23
1004c190c:      cmp x10, x1
1004c1910:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1914:      cmp x12, #0x3
1004c1918:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c191c:      ldr x10, [sp, #0x78]
1004c1920:      mov x9, x24
1004c1924:      cmp x10, x1
1004c1928:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c192c:      cmp x12, #0x4
1004c1930:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1934:      ldr x10, [sp, #0x88]
1004c1938:      mov x9, x25
1004c193c:      cmp x10, x1
1004c1940:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1944:      cmp x12, #0x5
1004c1948:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c194c:      ldr x10, [sp, #0x98]
1004c1950:      mov x9, x26
1004c1954:      cmp x10, x1
1004c1958:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c195c:      cmp x12, #0x6
1004c1960:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1964:      ldr x10, [sp, #0xa8]
1004c1968:      mov x9, x27
1004c196c:      cmp x10, x1
1004c1970:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1974:      cmp x12, #0x7
1004c1978:      b.eq    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c197c:      ldr x10, [sp, #0xb8]
1004c1980:      mov x9, x28
1004c1984:      cmp x10, x1
1004c1988:      b.eq    0x1004c1888 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c198c:      cmp x12, #0x8
1004c1990:      b.ne    0x1004c1898 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1994:      b   0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1998:      sub x13, x9, #0x1
1004c199c:      cmp x13, x14
1004c19a0:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c19a4:      cmp x10, #0x7
1004c19a8:      b.hi    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c19ac:      cmp w15, #0x22
1004c19b0:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c19b4:      sub x13, x9, #0x2
1004c19b8:      add x9, x0, #0x1
1004c19bc:      cmp x13, x14
1004c19c0:      b.ne    0x1004c1a1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c19c4:      sub x11, x10, #0x2
1004c19c8:      add x10, x20, x22
1004c19cc:      add x8, sp, #0xd0
1004c19d0:      add x9, x20, x22
1004c19d4:      stp x9, x11, [sp, #0x8]
1004c19d8:      add x0, x10, #0x1
1004c19dc:      str x1, [sp, #0x18]
1004c19e0:      mov x1, x11
1004c19e4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004c19e8:      ldr x11, [sp, #0x30]
1004c19ec:      ldr x8, [sp, #0xd0]
1004c19f0:      cbnz    x8, 0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c19f4:      ldr x8, [sp, #0x10]
1004c19f8:      cmp x8, #0x2
1004c19fc:      b.gt    0x1004c1a9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x57c>
1004c1a00:      cbz x8, 0x1004c1b04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5e4>
1004c1a04:      cmp x8, #0x1
1004c1a08:      b.ne    0x1004c1b30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x610>
1004c1a0c:      ldr x8, [sp, #0x8]
1004c1a10:      ldurb   w8, [x8, #0x1]
1004c1a14:      mov x9, #0x10000000000      ; =1099511627776
1004c1a18:      b   0x1004c1b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c1a1c:      ldrb    w13, [x9, x8]
1004c1a20:      cmp w13, #0x20
1004c1a24:      b.lo    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a28:      cmp w13, #0x22
1004c1a2c:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a30:      cmp w13, #0x5c
1004c1a34:      b.eq    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a38:      add x9, x9, #0x1
1004c1a3c:      subs    x12, x12, #0x1
1004c1a40:      b.ne    0x1004c1a1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c1a44:      b   0x1004c19c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4a4>
1004c1a48:      cmp w13, #0x74
1004c1a4c:      b.eq    0x1004c1abc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x59c>
1004c1a50:      cmp w13, #0x6e
1004c1a54:      b.ne    0x1004c1838 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c1a58:      add x8, x20, x22
1004c1a5c:      ldrb    w9, [x8, #0x1]
1004c1a60:      cmp w9, #0x75
1004c1a64:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a68:      ldrb    w8, [x8, #0x2]
1004c1a6c:      cmp w8, #0x6c
1004c1a70:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a74:      add x8, x20, x22
1004c1a78:      ldrb    w8, [x8, #0x3]
1004c1a7c:      cmp w8, #0x6c
1004c1a80:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1a84:      mov x8, #0x2                ; =2
1004c1a88:      movk    x8, #0x7ffc, lsl #48
1004c1a8c:      ldp x22, x12, [sp, #0x20]
1004c1a90:      cmp x12, #0x9
1004c1a94:      b.lo    0x1004c1874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c1a98:      b   0x1004c1b80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1a9c:      cmp x8, #0x3
1004c1aa0:      b.eq    0x1004c1b0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5ec>
1004c1aa4:      cmp x8, #0x4
1004c1aa8:      b.ne    0x1004c1b4c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x62c>
1004c1aac:      ldr x8, [sp, #0x8]
1004c1ab0:      ldur    w8, [x8, #0x1]
1004c1ab4:      mov x9, #0x40000000000      ; =4398046511104
1004c1ab8:      b   0x1004c1b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c1abc:      add x8, x20, x22
1004c1ac0:      ldrb    w9, [x8, #0x1]
1004c1ac4:      cmp w9, #0x72
1004c1ac8:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1acc:      ldrb    w8, [x8, #0x2]
1004c1ad0:      cmp w8, #0x75
1004c1ad4:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ad8:      add x8, x20, x22
1004c1adc:      ldrb    w8, [x8, #0x3]
1004c1ae0:      cmp w8, #0x65
1004c1ae4:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ae8:      mov x8, #0x2                ; =2
1004c1aec:      movk    x8, #0x7ffc, lsl #48
1004c1af0:      add x8, x8, #0x2
1004c1af4:      ldp x22, x12, [sp, #0x20]
1004c1af8:      cmp x12, #0x9
1004c1afc:      b.lo    0x1004c1874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c1b00:      b   0x1004c1b80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1b04:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004c1b08:      b   0x1004c1b6c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x64c>
1004c1b0c:      add x8, x20, x22
1004c1b10:      ldurb   w8, [x8, #0x1]
1004c1b14:      add x9, x20, x22
1004c1b18:      ldrb    w10, [x9, #0x2]
1004c1b1c:      ldrb    w9, [x9, #0x3]
1004c1b20:      orr x8, x8, x10, lsl #8
1004c1b24:      orr x8, x8, x9, lsl #16
1004c1b28:      mov x9, #0x30000000000      ; =3298534883328
1004c1b2c:      b   0x1004c1b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c1b30:      add x8, x20, x22
1004c1b34:      ldurb   w8, [x8, #0x1]
1004c1b38:      add x9, x20, x22
1004c1b3c:      ldrb    w9, [x9, #0x2]
1004c1b40:      orr x8, x8, x9, lsl #8
1004c1b44:      mov x9, #0x20000000000      ; =2199023255552
1004c1b48:      b   0x1004c1b64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c1b4c:      add x8, x20, x22
1004c1b50:      ldur    w8, [x8, #0x1]
1004c1b54:      add x9, x20, x22
1004c1b58:      ldrb    w9, [x9, #0x5]
1004c1b5c:      orr x8, x8, x9, lsl #32
1004c1b60:      mov x9, #0x50000000000      ; =5497558138880
1004c1b64:      movk    x9, #0x7ff9, lsl #48
1004c1b68:      orr x8, x8, x9
1004c1b6c:      ldp x11, x0, [sp, #0x30]
1004c1b70:      ldp x1, x22, [sp, #0x18]
1004c1b74:      ldr x12, [sp, #0x28]
1004c1b78:      cmp x12, #0x9
1004c1b7c:      b.lo    0x1004c1874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c1b80:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c1b84:      add x3, x3, #0xfb8
1004c1b88:      mov x0, #0x0                ; =0
1004c1b8c:      mov x1, x12
1004c1b90:      mov w2, #0x8                ; =8
1004c1b94:      bl  0x100ab0ecc <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c1b98:      str xzr, [x0]
1004c1b9c:      ldp x29, x30, [sp, #0x1b0]
1004c1ba0:      ldp x20, x19, [sp, #0x1a0]
1004c1ba4:      ldp x22, x21, [sp, #0x190]
1004c1ba8:      ldp x24, x23, [sp, #0x180]
1004c1bac:      ldp x26, x25, [sp, #0x170]
1004c1bb0:      ldp x28, x27, [sp, #0x160]
1004c1bb4:      add sp, sp, #0x1c0
1004c1bb8:      ret
1004c1bbc:      ldr x8, [sp, #0x30]
1004c1bc0:      str xzr, [x8]
1004c1bc4:      b   0x1004c1b9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c1bc8:      cmp w8, #0x7d
1004c1bcc:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1bd0:      add x8, x20, #0x1
1004c1bd4:      cmp x8, x19
1004c1bd8:      b.hs    0x1004c1c48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c1bdc:      mov x9, #0x2600             ; =9728
1004c1be0:      movk    x9, #0x1, lsl #32
1004c1be4:      ldrb    w10, [x0, x8]
1004c1be8:      cmp w10, #0x20
1004c1bec:      b.hi    0x1004c1c48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c1bf0:      lsr x10, x9, x10
1004c1bf4:      tbz w10, #0x0, 0x1004c1c48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c1bf8:      add x8, x8, #0x1
1004c1bfc:      cmp x19, x8
1004c1c00:      b.ne    0x1004c1be4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6c4>
1004c1c04:      ldr x8, [sp, #0xc8]
1004c1c08:      str x8, [sp, #0x150]
1004c1c0c:      ldur    q0, [sp, #0x88]
1004c1c10:      ldur    q1, [sp, #0x98]
1004c1c14:      stp q0, q1, [sp, #0x110]
1004c1c18:      ldur    q0, [sp, #0xa8]
1004c1c1c:      ldur    q1, [sp, #0xb8]
1004c1c20:      stp q0, q1, [sp, #0x130]
1004c1c24:      ldur    q0, [sp, #0x48]
1004c1c28:      ldur    q1, [sp, #0x58]
1004c1c2c:      stp q0, q1, [sp, #0xd0]
1004c1c30:      ldur    q0, [sp, #0x68]
1004c1c34:      ldur    q1, [sp, #0x78]
1004c1c38:      stp q0, q1, [sp, #0xf0]
1004c1c3c:      b   0x1004c1c88 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x768>
1004c1c40:      str xzr, [x11]
1004c1c44:      b   0x1004c1b9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c1c48:      ldr x9, [sp, #0xc8]
1004c1c4c:      str x9, [sp, #0x150]
1004c1c50:      ldur    q0, [sp, #0x88]
1004c1c54:      ldur    q1, [sp, #0x98]
1004c1c58:      stp q0, q1, [sp, #0x110]
1004c1c5c:      ldur    q0, [sp, #0xa8]
1004c1c60:      ldur    q1, [sp, #0xb8]
1004c1c64:      stp q0, q1, [sp, #0x130]
1004c1c68:      ldur    q0, [sp, #0x48]
1004c1c6c:      ldur    q1, [sp, #0x58]
1004c1c70:      stp q0, q1, [sp, #0xd0]
1004c1c74:      ldur    q0, [sp, #0x68]
1004c1c78:      ldur    q1, [sp, #0x78]
1004c1c7c:      stp q0, q1, [sp, #0xf0]
1004c1c80:      cmp x8, x19
1004c1c84:      b.ne    0x1004c1cc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7a8>
1004c1c88:      ldp q0, q1, [sp, #0x110]
1004c1c8c:      stur    q0, [x11, #0x48]
1004c1c90:      stur    q1, [x11, #0x58]
1004c1c94:      ldp q0, q1, [sp, #0x130]
1004c1c98:      stur    q0, [x11, #0x68]
1004c1c9c:      stur    q1, [x11, #0x78]
1004c1ca0:      ldp q0, q1, [sp, #0xd0]
1004c1ca4:      stur    q0, [x11, #0x8]
1004c1ca8:      stur    q1, [x11, #0x18]
1004c1cac:      ldp q0, q1, [sp, #0xf0]
1004c1cb0:      stur    q0, [x11, #0x28]
1004c1cb4:      ldr x8, [sp, #0x150]
1004c1cb8:      str x8, [x11, #0x88]
1004c1cbc:      mov w8, #0x1                ; =1
1004c1cc0:      stur    q1, [x11, #0x38]
1004c1cc4:      b   0x1004c1ccc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7ac>
1004c1cc8:      mov x8, #0x0                ; =0
1004c1ccc:      str x8, [x11]
1004c1cd0:      b   0x1004c1b9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c1cd4:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c1cd8:      add x3, x3, #0xfa0
1004c1cdc:      mov x0, x8
1004c1ce0:      mov x1, x9
1004c1ce4:      mov x2, x19
1004c1ce8:      bl  0x100ab0ecc <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
