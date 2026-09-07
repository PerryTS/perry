/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/nested-records-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100891724 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>:
100891724:      sub sp, sp, #0xb0
100891728:      stp d9, d8, [sp, #0x40]
10089172c:      stp x28, x27, [sp, #0x50]
100891730:      stp x26, x25, [sp, #0x60]
100891734:      stp x24, x23, [sp, #0x70]
100891738:      stp x22, x21, [sp, #0x80]
10089173c:      stp x20, x19, [sp, #0x90]
100891740:      stp x29, x30, [sp, #0xa0]
100891744:      add x29, sp, #0xa0
100891748:      mov x19, x1
10089174c:      mov x20, x0
100891750:      adrp    x1, 0x100dcc000 <_anon.48b093efb94be76b63af2132fa32e7b6.1032+0x155>
100891754:      add x1, x1, #0x180
100891758:      mov x0, sp
10089175c:      mov w2, #0x40               ; =64
100891760:      bl  0x100ce5990 <_writev+0x100ce5990>
100891764:      ldp x9, x8, [x20, #0x30]
100891768:      cmp x8, x9
10089176c:      b.hs    0x1008917d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
100891770:      ldr x9, [x20, #0x28]
100891774:      ldrb    w9, [x9, x8]
100891778:      cmp w9, #0x5d
10089177c:      b.ne    0x1008917d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
100891780:      add x8, x8, #0x1
100891784:      str x8, [x20, #0x38]
100891788:      mov w0, #0x0                ; =0
10089178c:      bl  0x1008cfa34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
100891790:      mov x20, x0
100891794:      bl  0x1008d14b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
100891798:      adrp    x0, 0x101138000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3box18BOOL_BOX_FREE_HEAD7STORAGE0s_023___RUST_STD_INTERNAL_VAL>
10089179c:      add x0, x0, #0x2a0
1008917a0:      ldr x8, [x0]
1008917a4:      blr x8
1008917a8:      ldrb    w8, [x0, #0x20]
1008917ac:      cbnz    w8, 0x100891b08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3e4>
1008917b0:      ldr x8, [x0]
1008917b4:      cbnz    x8, 0x100891b30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x40c>
1008917b8:      ldr x8, [x0, #0x18]
1008917bc:      cmp x19, x8
1008917c0:      b.hi    0x1008917c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xa4>
1008917c4:      str x19, [x0, #0x18]
1008917c8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1008917cc:      bfxil   x0, x20, #0, #48
1008917d0:      b   0x1008919cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2a8>
1008917d4:      mov x0, x20
1008917d8:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1008917dc:      ldrb    w8, [x20, #0x90]
1008917e0:      tbz w8, #0x0, 0x10089182c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x108>
1008917e4:      str x0, [sp]
1008917e8:      ldp x9, x8, [x20, #0x30]
1008917ec:      cmp x8, x9
1008917f0:      b.hs    0x100891870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
1008917f4:      ldr x10, [x20, #0x28]
1008917f8:      mov x11, #0x2600            ; =9728
1008917fc:      movk    x11, #0x1, lsl #32
100891800:      mov w1, #0x1                ; =1
100891804:      ldrb    w12, [x10, x8]
100891808:      cmp w12, #0x20
10089180c:      b.hi    0x100891870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
100891810:      lsr x12, x11, x12
100891814:      tbz w12, #0x0, 0x100891870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
100891818:      add x8, x8, #0x1
10089181c:      str x8, [x20, #0x38]
100891820:      cmp x9, x8
100891824:      b.ne    0x100891804 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xe0>
100891828:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089182c:      mov x1, #0x0                ; =0
100891830:      ldp x9, x8, [x20, #0x30]
100891834:      cmp x8, x9
100891838:      b.hs    0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089183c:      ldr x10, [x20, #0x28]
100891840:      mov x11, #0x2600            ; =9728
100891844:      movk    x11, #0x1, lsl #32
100891848:      ldrb    w12, [x10, x8]
10089184c:      cmp w12, #0x20
100891850:      b.hi    0x100891924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
100891854:      lsr x13, x11, x12
100891858:      tbz w13, #0x0, 0x100891924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10089185c:      add x8, x8, #0x1
100891860:      str x8, [x20, #0x38]
100891864:      cmp x9, x8
100891868:      b.ne    0x100891848 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x124>
10089186c:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891870:      cmp x8, x9
100891874:      b.hs    0x1008918e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1c4>
100891878:      ldr x10, [x20, #0x28]
10089187c:      ldrb    w11, [x10, x8]
100891880:      cmp w11, #0x2c
100891884:      b.ne    0x1008918f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1cc>
100891888:      add x8, x8, #0x1
10089188c:      str x8, [x20, #0x38]
100891890:      mov x0, x20
100891894:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891898:      ldrb    w8, [x20, #0x90]
10089189c:      cbz w8, 0x100891938 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x214>
1008918a0:      str x0, [sp, #0x8]
1008918a4:      ldp x9, x8, [x20, #0x30]
1008918a8:      cmp x8, x9
1008918ac:      b.hs    0x100891940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1008918b0:      ldr x10, [x20, #0x28]
1008918b4:      mov x11, #0x2600            ; =9728
1008918b8:      movk    x11, #0x1, lsl #32
1008918bc:      mov w1, #0x2                ; =2
1008918c0:      ldrb    w12, [x10, x8]
1008918c4:      cmp w12, #0x20
1008918c8:      b.hi    0x100891940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1008918cc:      lsr x12, x11, x12
1008918d0:      tbz w12, #0x0, 0x100891940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1008918d4:      add x8, x8, #0x1
1008918d8:      str x8, [x20, #0x38]
1008918dc:      cmp x9, x8
1008918e0:      b.ne    0x1008918c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x19c>
1008918e4:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1008918e8:      mov w1, #0x1                ; =1
1008918ec:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1008918f0:      mov w1, #0x1                ; =1
1008918f4:      mov x11, #0x2600            ; =9728
1008918f8:      movk    x11, #0x1, lsl #32
1008918fc:      ldrb    w12, [x10, x8]
100891900:      cmp w12, #0x20
100891904:      b.hi    0x100891924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
100891908:      lsr x13, x11, x12
10089190c:      tbz w13, #0x0, 0x100891924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
100891910:      add x8, x8, #0x1
100891914:      str x8, [x20, #0x38]
100891918:      cmp x9, x8
10089191c:      b.ne    0x1008918fc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d8>
100891920:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891924:      cmp w12, #0x5d
100891928:      b.ne    0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089192c:      add x8, x8, #0x1
100891930:      str x8, [x20, #0x38]
100891934:      b   0x1008919c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x29c>
100891938:      mov w1, #0x1                ; =1
10089193c:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891940:      cmp x8, x9
100891944:      b.hs    0x1008919b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x294>
100891948:      ldr x10, [x20, #0x28]
10089194c:      ldrb    w11, [x10, x8]
100891950:      cmp w11, #0x2c
100891954:      b.ne    0x1008919f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2cc>
100891958:      add x8, x8, #0x1
10089195c:      str x8, [x20, #0x38]
100891960:      mov x0, x20
100891964:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891968:      ldrb    w8, [x20, #0x90]
10089196c:      cbz w8, 0x1008919f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2d4>
100891970:      str x0, [sp, #0x10]
100891974:      ldp x9, x8, [x20, #0x30]
100891978:      cmp x8, x9
10089197c:      b.hs    0x100891a00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
100891980:      ldr x10, [x20, #0x28]
100891984:      mov x11, #0x2600            ; =9728
100891988:      movk    x11, #0x1, lsl #32
10089198c:      mov w1, #0x3                ; =3
100891990:      ldrb    w12, [x10, x8]
100891994:      cmp w12, #0x20
100891998:      b.hi    0x100891a00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10089199c:      lsr x12, x11, x12
1008919a0:      tbz w12, #0x0, 0x100891a00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
1008919a4:      add x8, x8, #0x1
1008919a8:      str x8, [x20, #0x38]
1008919ac:      cmp x9, x8
1008919b0:      b.ne    0x100891990 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x26c>
1008919b4:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1008919b8:      mov w1, #0x2                ; =2
1008919bc:      strb    wzr, [x20, #0x90]
1008919c0:      mov x0, sp
1008919c4:      mov x2, x19
1008919c8:      bl  0x100891474 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>
1008919cc:      ldp x29, x30, [sp, #0xa0]
1008919d0:      ldp x20, x19, [sp, #0x90]
1008919d4:      ldp x22, x21, [sp, #0x80]
1008919d8:      ldp x24, x23, [sp, #0x70]
1008919dc:      ldp x26, x25, [sp, #0x60]
1008919e0:      ldp x28, x27, [sp, #0x50]
1008919e4:      ldp d9, d8, [sp, #0x40]
1008919e8:      add sp, sp, #0xb0
1008919ec:      ret
1008919f0:      mov w1, #0x2                ; =2
1008919f4:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1008919f8:      mov w1, #0x2                ; =2
1008919fc:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891a00:      cmp x8, x9
100891a04:      b.hs    0x100891a78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x354>
100891a08:      ldr x10, [x20, #0x28]
100891a0c:      ldrb    w11, [x10, x8]
100891a10:      cmp w11, #0x2c
100891a14:      b.ne    0x100891a80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x35c>
100891a18:      add x8, x8, #0x1
100891a1c:      str x8, [x20, #0x38]
100891a20:      mov x0, x20
100891a24:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891a28:      ldrb    w8, [x20, #0x90]
100891a2c:      cbz w8, 0x100891a88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x364>
100891a30:      str x0, [sp, #0x18]
100891a34:      ldp x9, x8, [x20, #0x30]
100891a38:      cmp x8, x9
100891a3c:      b.hs    0x100891a90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
100891a40:      ldr x10, [x20, #0x28]
100891a44:      mov x11, #0x2600            ; =9728
100891a48:      movk    x11, #0x1, lsl #32
100891a4c:      mov w1, #0x4                ; =4
100891a50:      ldrb    w12, [x10, x8]
100891a54:      cmp w12, #0x20
100891a58:      b.hi    0x100891a90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
100891a5c:      lsr x12, x11, x12
100891a60:      tbz w12, #0x0, 0x100891a90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
100891a64:      add x8, x8, #0x1
100891a68:      str x8, [x20, #0x38]
100891a6c:      cmp x9, x8
100891a70:      b.ne    0x100891a50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x32c>
100891a74:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891a78:      mov w1, #0x3                ; =3
100891a7c:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891a80:      mov w1, #0x3                ; =3
100891a84:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891a88:      mov w1, #0x3                ; =3
100891a8c:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891a90:      cmp x8, x9
100891a94:      b.hs    0x100891b3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x418>
100891a98:      ldr x10, [x20, #0x28]
100891a9c:      ldrb    w11, [x10, x8]
100891aa0:      cmp w11, #0x2c
100891aa4:      b.ne    0x100891b50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x42c>
100891aa8:      add x8, x8, #0x1
100891aac:      str x8, [x20, #0x38]
100891ab0:      mov x0, x20
100891ab4:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891ab8:      ldrb    w8, [x20, #0x90]
100891abc:      cbz w8, 0x100891b58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x434>
100891ac0:      str x0, [sp, #0x20]
100891ac4:      ldp x9, x8, [x20, #0x30]
100891ac8:      cmp x8, x9
100891acc:      b.hs    0x100891b60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
100891ad0:      ldr x10, [x20, #0x28]
100891ad4:      mov x11, #0x2600            ; =9728
100891ad8:      movk    x11, #0x1, lsl #32
100891adc:      mov w1, #0x5                ; =5
100891ae0:      ldrb    w12, [x10, x8]
100891ae4:      cmp w12, #0x20
100891ae8:      b.hi    0x100891b60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
100891aec:      lsr x12, x11, x12
100891af0:      tbz w12, #0x0, 0x100891b60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
100891af4:      add x8, x8, #0x1
100891af8:      str x8, [x20, #0x38]
100891afc:      cmp x9, x8
100891b00:      b.ne    0x100891ae0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3bc>
100891b04:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891b08:      cmp w8, #0x1
100891b0c:      b.ne    0x100891b44 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x420>
100891b10:      adrp    x1, 0x100250000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe4>
100891b14:      add x1, x1, #0xeec
100891b18:      mov x21, x0
100891b1c:      bl  0x100ba7e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100891b20:      mov x0, x21
100891b24:      strb    wzr, [x21, #0x20]
100891b28:      ldr x8, [x21]
100891b2c:      cbz x8, 0x1008917b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x94>
100891b30:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
100891b34:      add x0, x0, #0xe58
100891b38:      bl  0x100c99c2c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100891b3c:      mov w1, #0x4                ; =4
100891b40:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891b44:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
100891b48:      add x0, x0, #0xed8
100891b4c:      bl  0x100cdc11c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100891b50:      mov w1, #0x4                ; =4
100891b54:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891b58:      mov w1, #0x4                ; =4
100891b5c:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891b60:      cmp x8, x9
100891b64:      b.hs    0x100891bd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4b4>
100891b68:      ldr x10, [x20, #0x28]
100891b6c:      ldrb    w11, [x10, x8]
100891b70:      cmp w11, #0x2c
100891b74:      b.ne    0x100891be0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4bc>
100891b78:      add x8, x8, #0x1
100891b7c:      str x8, [x20, #0x38]
100891b80:      mov x0, x20
100891b84:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891b88:      ldrb    w8, [x20, #0x90]
100891b8c:      cbz w8, 0x100891be8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4c4>
100891b90:      str x0, [sp, #0x28]
100891b94:      ldp x9, x8, [x20, #0x30]
100891b98:      cmp x8, x9
100891b9c:      b.hs    0x100891bf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
100891ba0:      ldr x10, [x20, #0x28]
100891ba4:      mov x11, #0x2600            ; =9728
100891ba8:      movk    x11, #0x1, lsl #32
100891bac:      mov w1, #0x6                ; =6
100891bb0:      ldrb    w12, [x10, x8]
100891bb4:      cmp w12, #0x20
100891bb8:      b.hi    0x100891bf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
100891bbc:      lsr x12, x11, x12
100891bc0:      tbz w12, #0x0, 0x100891bf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
100891bc4:      add x8, x8, #0x1
100891bc8:      str x8, [x20, #0x38]
100891bcc:      cmp x9, x8
100891bd0:      b.ne    0x100891bb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x48c>
100891bd4:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891bd8:      mov w1, #0x5                ; =5
100891bdc:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891be0:      mov w1, #0x5                ; =5
100891be4:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891be8:      mov w1, #0x5                ; =5
100891bec:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891bf0:      cmp x8, x9
100891bf4:      b.hs    0x100891c68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x544>
100891bf8:      ldr x10, [x20, #0x28]
100891bfc:      ldrb    w11, [x10, x8]
100891c00:      cmp w11, #0x2c
100891c04:      b.ne    0x100891c70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x54c>
100891c08:      add x8, x8, #0x1
100891c0c:      str x8, [x20, #0x38]
100891c10:      mov x0, x20
100891c14:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891c18:      ldrb    w8, [x20, #0x90]
100891c1c:      cbz w8, 0x100891c78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x554>
100891c20:      str x0, [sp, #0x30]
100891c24:      ldp x9, x8, [x20, #0x30]
100891c28:      cmp x8, x9
100891c2c:      b.hs    0x100891c80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
100891c30:      ldr x10, [x20, #0x28]
100891c34:      mov x11, #0x2600            ; =9728
100891c38:      movk    x11, #0x1, lsl #32
100891c3c:      mov w1, #0x7                ; =7
100891c40:      ldrb    w12, [x10, x8]
100891c44:      cmp w12, #0x20
100891c48:      b.hi    0x100891c80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
100891c4c:      lsr x12, x11, x12
100891c50:      tbz w12, #0x0, 0x100891c80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
100891c54:      add x8, x8, #0x1
100891c58:      str x8, [x20, #0x38]
100891c5c:      cmp x9, x8
100891c60:      b.ne    0x100891c40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x51c>
100891c64:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891c68:      mov w1, #0x6                ; =6
100891c6c:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891c70:      mov w1, #0x6                ; =6
100891c74:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891c78:      mov w1, #0x6                ; =6
100891c7c:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891c80:      cmp x8, x9
100891c84:      b.hs    0x100891cf8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5d4>
100891c88:      ldr x10, [x20, #0x28]
100891c8c:      ldrb    w11, [x10, x8]
100891c90:      cmp w11, #0x2c
100891c94:      b.ne    0x100891d00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5dc>
100891c98:      add x8, x8, #0x1
100891c9c:      str x8, [x20, #0x38]
100891ca0:      mov x0, x20
100891ca4:      bl  0x1008909a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
100891ca8:      ldrb    w8, [x20, #0x90]
100891cac:      cbz w8, 0x100891d08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5e4>
100891cb0:      str x0, [sp, #0x38]
100891cb4:      ldp x9, x8, [x20, #0x30]
100891cb8:      cmp x8, x9
100891cbc:      b.hs    0x100891d10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
100891cc0:      ldr x10, [x20, #0x28]
100891cc4:      mov x11, #0x2600            ; =9728
100891cc8:      movk    x11, #0x1, lsl #32
100891ccc:      mov w1, #0x8                ; =8
100891cd0:      ldrb    w12, [x10, x8]
100891cd4:      cmp w12, #0x20
100891cd8:      b.hi    0x100891d10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
100891cdc:      lsr x12, x11, x12
100891ce0:      tbz w12, #0x0, 0x100891d10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
100891ce4:      add x8, x8, #0x1
100891ce8:      str x8, [x20, #0x38]
100891cec:      cmp x9, x8
100891cf0:      b.ne    0x100891cd0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ac>
100891cf4:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891cf8:      mov w1, #0x7                ; =7
100891cfc:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891d00:      mov w1, #0x7                ; =7
100891d04:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891d08:      mov w1, #0x7                ; =7
100891d0c:      b   0x100891830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
100891d10:      cmp x8, x9
100891d14:      b.hs    0x100891ed8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7b4>
100891d18:      ldr x10, [x20, #0x28]
100891d1c:      ldrb    w11, [x10, x8]
100891d20:      cmp w11, #0x2c
100891d24:      b.ne    0x100891ee0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7bc>
100891d28:      add x8, x8, #0x1
100891d2c:      str x8, [x20, #0x38]
100891d30:      mov w0, #0x10               ; =16
100891d34:      bl  0x1008f7cc0 <_js_array_alloc>
100891d38:      mov x21, x0
100891d3c:      mov x25, #0x0               ; =0
100891d40:      mov x27, sp
100891d44:      mov w28, #0x7ffe            ; =32766
100891d48:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
100891d4c:      fmov    d8, x8
100891d50:      lsr x26, x0, #3
100891d54:      b   0x100891d6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
100891d58:      add w8, w22, #0x1
100891d5c:      str w8, [x21]
100891d60:      add x25, x25, #0x8
100891d64:      cmp x25, #0x40
100891d68:      b.eq    0x100891ee8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
100891d6c:      ldr x23, [x27, x25]
100891d70:      ldp w22, w8, [x21]
100891d74:      cmp w22, w8
100891d78:      b.hs    0x100891db0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x68c>
100891d7c:      add x8, x21, #0x8
100891d80:      add x24, x8, x22, lsl #3
100891d84:      str x23, [x24]
100891d88:      mov x0, x21
100891d8c:      bl  0x1008d05c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
100891d90:      tbz w0, #0x0, 0x100891dcc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6a8>
100891d94:      cmp x28, x23, lsr #48
100891d98:      b.ne    0x100891dec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6c8>
100891d9c:      mov x0, x23
100891da0:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
100891da4:      tbnz    w0, #0x0, 0x100891e08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
100891da8:      scvtf   d0, w23
100891dac:      b   0x100891e04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e0>
100891db0:      fmov    d0, x23
100891db4:      mov x0, x21
100891db8:      bl  0x100615c88 <_js_array_push_f64>
100891dbc:      add x25, x25, #0x8
100891dc0:      cmp x25, #0x40
100891dc4:      b.ne    0x100891d6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
100891dc8:      b   0x100891ee8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
100891dcc:      cmp x26, #0x201
100891dd0:      b.lo    0x100891e08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
100891dd4:      ldurb   w8, [x21, #-0x8]
100891dd8:      cmp w8, #0x1
100891ddc:      b.ne    0x100891e08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
100891de0:      ldurh   w8, [x21, #-0x6]
100891de4:      tbnz    w8, #0xc, 0x100891d94 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x670>
100891de8:      b   0x100891e08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
100891dec:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
100891df0:      cmp x23, x8
100891df4:      b.gt    0x100891e08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
100891df8:      fmov    d0, x23
100891dfc:      fcmp    d0, d0
100891e00:      fcsel   d0, d8, d0, vs
100891e04:      fmov    x23, d0
100891e08:      str x23, [x24]
100891e0c:      cmp x28, x23, lsr #48
100891e10:      b.ne    0x100891e28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x704>
100891e14:      mov x0, x23
100891e18:      bl  0x100845448 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
100891e1c:      tbnz    w0, #0x0, 0x100891e34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x710>
100891e20:      scvtf   d0, w23
100891e24:      b   0x100891e7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x758>
100891e28:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
100891e2c:      cmp x23, x8
100891e30:      b.le    0x100891e70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x74c>
100891e34:      cmp x26, #0x201
100891e38:      b.lo    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e3c:      ldurb   w8, [x21, #-0x8]
100891e40:      cmp w8, #0x1
100891e44:      b.ne    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e48:      ldurh   w8, [x21, #-0x6]
100891e4c:      mov w9, #0xef7f             ; =61311
100891e50:      and w9, w8, w9
100891e54:      sturh   w9, [x21, #-0x6]
100891e58:      mov w9, #0x1080             ; =4224
100891e5c:      tst w8, w9
100891e60:      b.eq    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e64:      mov x0, x21
100891e68:      bl  0x1005aaee0 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
100891e6c:      b   0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e70:      fmov    d0, x23
100891e74:      fcmp    d0, d0
100891e78:      fcsel   d0, d8, d0, vs
100891e7c:      cmp x26, #0x201
100891e80:      b.lo    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e84:      ldurb   w8, [x21, #-0x8]
100891e88:      cmp w8, #0x1
100891e8c:      b.ne    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e90:      ldurh   w8, [x21, #-0x6]
100891e94:      tbz w8, #0x7, 0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891e98:      ldr w8, [x21]
100891e9c:      cmp w22, w8
100891ea0:      b.hs    0x100891ea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
100891ea4:      str d0, [x24]
100891ea8:      mov x0, x21
100891eac:      mov x1, x22
100891eb0:      mov x2, x23
100891eb4:      bl  0x100319c40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
100891eb8:      mov x0, x21
100891ebc:      bl  0x1008c2b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
100891ec0:      cbz w0, 0x100891d58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
100891ec4:      mov x0, x21
100891ec8:      mov x1, x24
100891ecc:      mov x2, x23
100891ed0:      bl  0x1005b7284 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc13barrier_store26runtime_write_barrier_slot>
100891ed4:      b   0x100891d58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
100891ed8:      mov w1, #0x8                ; =8
100891edc:      b   0x1008919bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
100891ee0:      mov w1, #0x8                ; =8
100891ee4:      b   0x1008918f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
100891ee8:      mov x0, x20
100891eec:      mov x1, x21
100891ef0:      mov x2, x19
100891ef4:      ldp x29, x30, [sp, #0xa0]
100891ef8:      ldp x20, x19, [sp, #0x90]
100891efc:      ldp x22, x21, [sp, #0x80]
100891f00:      ldp x24, x23, [sp, #0x70]
100891f04:      ldp x26, x25, [sp, #0x60]
100891f08:      ldp x28, x27, [sp, #0x50]
100891f0c:      ldp d9, d8, [sp, #0x40]
100891f10:      add sp, sp, #0xb0
100891f14:      b   0x100890f18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
