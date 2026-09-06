/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-array-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010089a600 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>:
10089a600:      sub sp, sp, #0xb0
10089a604:      stp d9, d8, [sp, #0x40]
10089a608:      stp x28, x27, [sp, #0x50]
10089a60c:      stp x26, x25, [sp, #0x60]
10089a610:      stp x24, x23, [sp, #0x70]
10089a614:      stp x22, x21, [sp, #0x80]
10089a618:      stp x20, x19, [sp, #0x90]
10089a61c:      stp x29, x30, [sp, #0xa0]
10089a620:      add x29, sp, #0xa0
10089a624:      mov x19, x1
10089a628:      mov x20, x0
10089a62c:      adrp    x1, 0x100de1000 <_anon.6f01103757ba914c062b5a65e3f754d5.1335+0x56b>
10089a630:      add x1, x1, #0x130
10089a634:      mov x0, sp
10089a638:      mov w2, #0x40               ; =64
10089a63c:      bl  0x100ce4f90 <_writev+0x100ce4f90>
10089a640:      ldp x9, x8, [x20, #0x30]
10089a644:      cmp x8, x9
10089a648:      b.hs    0x10089a6b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10089a64c:      ldr x9, [x20, #0x28]
10089a650:      ldrb    w9, [x9, x8]
10089a654:      cmp w9, #0x5d
10089a658:      b.ne    0x10089a6b0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10089a65c:      add x8, x8, #0x1
10089a660:      str x8, [x20, #0x38]
10089a664:      mov w0, #0x0                ; =0
10089a668:      bl  0x1008dcf90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10089a66c:      mov x20, x0
10089a670:      bl  0x1008dea38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10089a674:      adrp    x0, 0x101134000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json17PARSE_SHAPE_CACHE0023___RUST_STD_INTERNAL_VAL>
10089a678:      add x0, x0, #0x660
10089a67c:      ldr x8, [x0]
10089a680:      blr x8
10089a684:      ldrb    w8, [x0, #0x20]
10089a688:      cbnz    w8, 0x10089a9e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3e4>
10089a68c:      ldr x8, [x0]
10089a690:      cbnz    x8, 0x10089aa0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x40c>
10089a694:      ldr x8, [x0, #0x18]
10089a698:      cmp x19, x8
10089a69c:      b.hi    0x10089a6a4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xa4>
10089a6a0:      str x19, [x0, #0x18]
10089a6a4:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10089a6a8:      bfxil   x0, x20, #0, #48
10089a6ac:      b   0x10089a8a8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2a8>
10089a6b0:      mov x0, x20
10089a6b4:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a6b8:      ldrb    w8, [x20, #0x90]
10089a6bc:      tbz w8, #0x0, 0x10089a708 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x108>
10089a6c0:      str x0, [sp]
10089a6c4:      ldp x9, x8, [x20, #0x30]
10089a6c8:      cmp x8, x9
10089a6cc:      b.hs    0x10089a74c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10089a6d0:      ldr x10, [x20, #0x28]
10089a6d4:      mov x11, #0x2600            ; =9728
10089a6d8:      movk    x11, #0x1, lsl #32
10089a6dc:      mov w1, #0x1                ; =1
10089a6e0:      ldrb    w12, [x10, x8]
10089a6e4:      cmp w12, #0x20
10089a6e8:      b.hi    0x10089a74c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10089a6ec:      lsr x12, x11, x12
10089a6f0:      tbz w12, #0x0, 0x10089a74c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10089a6f4:      add x8, x8, #0x1
10089a6f8:      str x8, [x20, #0x38]
10089a6fc:      cmp x9, x8
10089a700:      b.ne    0x10089a6e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xe0>
10089a704:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a708:      mov x1, #0x0                ; =0
10089a70c:      ldp x9, x8, [x20, #0x30]
10089a710:      cmp x8, x9
10089a714:      b.hs    0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a718:      ldr x10, [x20, #0x28]
10089a71c:      mov x11, #0x2600            ; =9728
10089a720:      movk    x11, #0x1, lsl #32
10089a724:      ldrb    w12, [x10, x8]
10089a728:      cmp w12, #0x20
10089a72c:      b.hi    0x10089a800 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10089a730:      lsr x13, x11, x12
10089a734:      tbz w13, #0x0, 0x10089a800 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10089a738:      add x8, x8, #0x1
10089a73c:      str x8, [x20, #0x38]
10089a740:      cmp x9, x8
10089a744:      b.ne    0x10089a724 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x124>
10089a748:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a74c:      cmp x8, x9
10089a750:      b.hs    0x10089a7c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1c4>
10089a754:      ldr x10, [x20, #0x28]
10089a758:      ldrb    w11, [x10, x8]
10089a75c:      cmp w11, #0x2c
10089a760:      b.ne    0x10089a7cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1cc>
10089a764:      add x8, x8, #0x1
10089a768:      str x8, [x20, #0x38]
10089a76c:      mov x0, x20
10089a770:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a774:      ldrb    w8, [x20, #0x90]
10089a778:      cbz w8, 0x10089a814 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x214>
10089a77c:      str x0, [sp, #0x8]
10089a780:      ldp x9, x8, [x20, #0x30]
10089a784:      cmp x8, x9
10089a788:      b.hs    0x10089a81c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10089a78c:      ldr x10, [x20, #0x28]
10089a790:      mov x11, #0x2600            ; =9728
10089a794:      movk    x11, #0x1, lsl #32
10089a798:      mov w1, #0x2                ; =2
10089a79c:      ldrb    w12, [x10, x8]
10089a7a0:      cmp w12, #0x20
10089a7a4:      b.hi    0x10089a81c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10089a7a8:      lsr x12, x11, x12
10089a7ac:      tbz w12, #0x0, 0x10089a81c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10089a7b0:      add x8, x8, #0x1
10089a7b4:      str x8, [x20, #0x38]
10089a7b8:      cmp x9, x8
10089a7bc:      b.ne    0x10089a79c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x19c>
10089a7c0:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a7c4:      mov w1, #0x1                ; =1
10089a7c8:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a7cc:      mov w1, #0x1                ; =1
10089a7d0:      mov x11, #0x2600            ; =9728
10089a7d4:      movk    x11, #0x1, lsl #32
10089a7d8:      ldrb    w12, [x10, x8]
10089a7dc:      cmp w12, #0x20
10089a7e0:      b.hi    0x10089a800 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10089a7e4:      lsr x13, x11, x12
10089a7e8:      tbz w13, #0x0, 0x10089a800 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10089a7ec:      add x8, x8, #0x1
10089a7f0:      str x8, [x20, #0x38]
10089a7f4:      cmp x9, x8
10089a7f8:      b.ne    0x10089a7d8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d8>
10089a7fc:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a800:      cmp w12, #0x5d
10089a804:      b.ne    0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a808:      add x8, x8, #0x1
10089a80c:      str x8, [x20, #0x38]
10089a810:      b   0x10089a89c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x29c>
10089a814:      mov w1, #0x1                ; =1
10089a818:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089a81c:      cmp x8, x9
10089a820:      b.hs    0x10089a894 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x294>
10089a824:      ldr x10, [x20, #0x28]
10089a828:      ldrb    w11, [x10, x8]
10089a82c:      cmp w11, #0x2c
10089a830:      b.ne    0x10089a8cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2cc>
10089a834:      add x8, x8, #0x1
10089a838:      str x8, [x20, #0x38]
10089a83c:      mov x0, x20
10089a840:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a844:      ldrb    w8, [x20, #0x90]
10089a848:      cbz w8, 0x10089a8d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2d4>
10089a84c:      str x0, [sp, #0x10]
10089a850:      ldp x9, x8, [x20, #0x30]
10089a854:      cmp x8, x9
10089a858:      b.hs    0x10089a8dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10089a85c:      ldr x10, [x20, #0x28]
10089a860:      mov x11, #0x2600            ; =9728
10089a864:      movk    x11, #0x1, lsl #32
10089a868:      mov w1, #0x3                ; =3
10089a86c:      ldrb    w12, [x10, x8]
10089a870:      cmp w12, #0x20
10089a874:      b.hi    0x10089a8dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10089a878:      lsr x12, x11, x12
10089a87c:      tbz w12, #0x0, 0x10089a8dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10089a880:      add x8, x8, #0x1
10089a884:      str x8, [x20, #0x38]
10089a888:      cmp x9, x8
10089a88c:      b.ne    0x10089a86c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x26c>
10089a890:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a894:      mov w1, #0x2                ; =2
10089a898:      strb    wzr, [x20, #0x90]
10089a89c:      mov x0, sp
10089a8a0:      mov x2, x19
10089a8a4:      bl  0x10089a350 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>
10089a8a8:      ldp x29, x30, [sp, #0xa0]
10089a8ac:      ldp x20, x19, [sp, #0x90]
10089a8b0:      ldp x22, x21, [sp, #0x80]
10089a8b4:      ldp x24, x23, [sp, #0x70]
10089a8b8:      ldp x26, x25, [sp, #0x60]
10089a8bc:      ldp x28, x27, [sp, #0x50]
10089a8c0:      ldp d9, d8, [sp, #0x40]
10089a8c4:      add sp, sp, #0xb0
10089a8c8:      ret
10089a8cc:      mov w1, #0x2                ; =2
10089a8d0:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089a8d4:      mov w1, #0x2                ; =2
10089a8d8:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089a8dc:      cmp x8, x9
10089a8e0:      b.hs    0x10089a954 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x354>
10089a8e4:      ldr x10, [x20, #0x28]
10089a8e8:      ldrb    w11, [x10, x8]
10089a8ec:      cmp w11, #0x2c
10089a8f0:      b.ne    0x10089a95c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x35c>
10089a8f4:      add x8, x8, #0x1
10089a8f8:      str x8, [x20, #0x38]
10089a8fc:      mov x0, x20
10089a900:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a904:      ldrb    w8, [x20, #0x90]
10089a908:      cbz w8, 0x10089a964 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x364>
10089a90c:      str x0, [sp, #0x18]
10089a910:      ldp x9, x8, [x20, #0x30]
10089a914:      cmp x8, x9
10089a918:      b.hs    0x10089a96c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10089a91c:      ldr x10, [x20, #0x28]
10089a920:      mov x11, #0x2600            ; =9728
10089a924:      movk    x11, #0x1, lsl #32
10089a928:      mov w1, #0x4                ; =4
10089a92c:      ldrb    w12, [x10, x8]
10089a930:      cmp w12, #0x20
10089a934:      b.hi    0x10089a96c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10089a938:      lsr x12, x11, x12
10089a93c:      tbz w12, #0x0, 0x10089a96c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10089a940:      add x8, x8, #0x1
10089a944:      str x8, [x20, #0x38]
10089a948:      cmp x9, x8
10089a94c:      b.ne    0x10089a92c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x32c>
10089a950:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a954:      mov w1, #0x3                ; =3
10089a958:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a95c:      mov w1, #0x3                ; =3
10089a960:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089a964:      mov w1, #0x3                ; =3
10089a968:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089a96c:      cmp x8, x9
10089a970:      b.hs    0x10089aa18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x418>
10089a974:      ldr x10, [x20, #0x28]
10089a978:      ldrb    w11, [x10, x8]
10089a97c:      cmp w11, #0x2c
10089a980:      b.ne    0x10089aa2c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x42c>
10089a984:      add x8, x8, #0x1
10089a988:      str x8, [x20, #0x38]
10089a98c:      mov x0, x20
10089a990:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089a994:      ldrb    w8, [x20, #0x90]
10089a998:      cbz w8, 0x10089aa34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x434>
10089a99c:      str x0, [sp, #0x20]
10089a9a0:      ldp x9, x8, [x20, #0x30]
10089a9a4:      cmp x8, x9
10089a9a8:      b.hs    0x10089aa3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10089a9ac:      ldr x10, [x20, #0x28]
10089a9b0:      mov x11, #0x2600            ; =9728
10089a9b4:      movk    x11, #0x1, lsl #32
10089a9b8:      mov w1, #0x5                ; =5
10089a9bc:      ldrb    w12, [x10, x8]
10089a9c0:      cmp w12, #0x20
10089a9c4:      b.hi    0x10089aa3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10089a9c8:      lsr x12, x11, x12
10089a9cc:      tbz w12, #0x0, 0x10089aa3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10089a9d0:      add x8, x8, #0x1
10089a9d4:      str x8, [x20, #0x38]
10089a9d8:      cmp x9, x8
10089a9dc:      b.ne    0x10089a9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3bc>
10089a9e0:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089a9e4:      cmp w8, #0x1
10089a9e8:      b.ne    0x10089aa20 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x420>
10089a9ec:      adrp    x1, 0x1006ee000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x7c>
10089a9f0:      add x1, x1, #0xd0
10089a9f4:      mov x21, x0
10089a9f8:      bl  0x100ba7c9c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10089a9fc:      mov x0, x21
10089aa00:      strb    wzr, [x21, #0x20]
10089aa04:      ldr x8, [x21]
10089aa08:      cbz x8, 0x10089a694 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x94>
10089aa0c:      adrp    x0, 0x10109c000 <_anon.438b28c8644b10f28676d307896bf03a.21>
10089aa10:      add x0, x0, #0xe58
10089aa14:      bl  0x100c99aac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10089aa18:      mov w1, #0x4                ; =4
10089aa1c:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089aa20:      adrp    x0, 0x10109b000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10089aa24:      add x0, x0, #0xed8
10089aa28:      bl  0x100cdb71c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10089aa2c:      mov w1, #0x4                ; =4
10089aa30:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089aa34:      mov w1, #0x4                ; =4
10089aa38:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089aa3c:      cmp x8, x9
10089aa40:      b.hs    0x10089aab4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4b4>
10089aa44:      ldr x10, [x20, #0x28]
10089aa48:      ldrb    w11, [x10, x8]
10089aa4c:      cmp w11, #0x2c
10089aa50:      b.ne    0x10089aabc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4bc>
10089aa54:      add x8, x8, #0x1
10089aa58:      str x8, [x20, #0x38]
10089aa5c:      mov x0, x20
10089aa60:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089aa64:      ldrb    w8, [x20, #0x90]
10089aa68:      cbz w8, 0x10089aac4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4c4>
10089aa6c:      str x0, [sp, #0x28]
10089aa70:      ldp x9, x8, [x20, #0x30]
10089aa74:      cmp x8, x9
10089aa78:      b.hs    0x10089aacc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10089aa7c:      ldr x10, [x20, #0x28]
10089aa80:      mov x11, #0x2600            ; =9728
10089aa84:      movk    x11, #0x1, lsl #32
10089aa88:      mov w1, #0x6                ; =6
10089aa8c:      ldrb    w12, [x10, x8]
10089aa90:      cmp w12, #0x20
10089aa94:      b.hi    0x10089aacc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10089aa98:      lsr x12, x11, x12
10089aa9c:      tbz w12, #0x0, 0x10089aacc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10089aaa0:      add x8, x8, #0x1
10089aaa4:      str x8, [x20, #0x38]
10089aaa8:      cmp x9, x8
10089aaac:      b.ne    0x10089aa8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x48c>
10089aab0:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089aab4:      mov w1, #0x5                ; =5
10089aab8:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089aabc:      mov w1, #0x5                ; =5
10089aac0:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089aac4:      mov w1, #0x5                ; =5
10089aac8:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089aacc:      cmp x8, x9
10089aad0:      b.hs    0x10089ab44 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x544>
10089aad4:      ldr x10, [x20, #0x28]
10089aad8:      ldrb    w11, [x10, x8]
10089aadc:      cmp w11, #0x2c
10089aae0:      b.ne    0x10089ab4c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x54c>
10089aae4:      add x8, x8, #0x1
10089aae8:      str x8, [x20, #0x38]
10089aaec:      mov x0, x20
10089aaf0:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089aaf4:      ldrb    w8, [x20, #0x90]
10089aaf8:      cbz w8, 0x10089ab54 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x554>
10089aafc:      str x0, [sp, #0x30]
10089ab00:      ldp x9, x8, [x20, #0x30]
10089ab04:      cmp x8, x9
10089ab08:      b.hs    0x10089ab5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10089ab0c:      ldr x10, [x20, #0x28]
10089ab10:      mov x11, #0x2600            ; =9728
10089ab14:      movk    x11, #0x1, lsl #32
10089ab18:      mov w1, #0x7                ; =7
10089ab1c:      ldrb    w12, [x10, x8]
10089ab20:      cmp w12, #0x20
10089ab24:      b.hi    0x10089ab5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10089ab28:      lsr x12, x11, x12
10089ab2c:      tbz w12, #0x0, 0x10089ab5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10089ab30:      add x8, x8, #0x1
10089ab34:      str x8, [x20, #0x38]
10089ab38:      cmp x9, x8
10089ab3c:      b.ne    0x10089ab1c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x51c>
10089ab40:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089ab44:      mov w1, #0x6                ; =6
10089ab48:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089ab4c:      mov w1, #0x6                ; =6
10089ab50:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089ab54:      mov w1, #0x6                ; =6
10089ab58:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089ab5c:      cmp x8, x9
10089ab60:      b.hs    0x10089abd4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5d4>
10089ab64:      ldr x10, [x20, #0x28]
10089ab68:      ldrb    w11, [x10, x8]
10089ab6c:      cmp w11, #0x2c
10089ab70:      b.ne    0x10089abdc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5dc>
10089ab74:      add x8, x8, #0x1
10089ab78:      str x8, [x20, #0x38]
10089ab7c:      mov x0, x20
10089ab80:      bl  0x10089987c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10089ab84:      ldrb    w8, [x20, #0x90]
10089ab88:      cbz w8, 0x10089abe4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5e4>
10089ab8c:      str x0, [sp, #0x38]
10089ab90:      ldp x9, x8, [x20, #0x30]
10089ab94:      cmp x8, x9
10089ab98:      b.hs    0x10089abec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10089ab9c:      ldr x10, [x20, #0x28]
10089aba0:      mov x11, #0x2600            ; =9728
10089aba4:      movk    x11, #0x1, lsl #32
10089aba8:      mov w1, #0x8                ; =8
10089abac:      ldrb    w12, [x10, x8]
10089abb0:      cmp w12, #0x20
10089abb4:      b.hi    0x10089abec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10089abb8:      lsr x12, x11, x12
10089abbc:      tbz w12, #0x0, 0x10089abec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10089abc0:      add x8, x8, #0x1
10089abc4:      str x8, [x20, #0x38]
10089abc8:      cmp x9, x8
10089abcc:      b.ne    0x10089abac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ac>
10089abd0:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089abd4:      mov w1, #0x7                ; =7
10089abd8:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089abdc:      mov w1, #0x7                ; =7
10089abe0:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089abe4:      mov w1, #0x7                ; =7
10089abe8:      b   0x10089a70c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10089abec:      cmp x8, x9
10089abf0:      b.hs    0x10089adb4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7b4>
10089abf4:      ldr x10, [x20, #0x28]
10089abf8:      ldrb    w11, [x10, x8]
10089abfc:      cmp w11, #0x2c
10089ac00:      b.ne    0x10089adbc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7bc>
10089ac04:      add x8, x8, #0x1
10089ac08:      str x8, [x20, #0x38]
10089ac0c:      mov w0, #0x10               ; =16
10089ac10:      bl  0x10091817c <_js_array_alloc>
10089ac14:      mov x21, x0
10089ac18:      mov x25, #0x0               ; =0
10089ac1c:      mov x27, sp
10089ac20:      mov w28, #0x7ffe            ; =32766
10089ac24:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10089ac28:      fmov    d8, x8
10089ac2c:      lsr x26, x0, #3
10089ac30:      b   0x10089ac48 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10089ac34:      add w8, w22, #0x1
10089ac38:      str w8, [x21]
10089ac3c:      add x25, x25, #0x8
10089ac40:      cmp x25, #0x40
10089ac44:      b.eq    0x10089adc4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10089ac48:      ldr x23, [x27, x25]
10089ac4c:      ldp w22, w8, [x21]
10089ac50:      cmp w22, w8
10089ac54:      b.hs    0x10089ac8c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x68c>
10089ac58:      add x8, x21, #0x8
10089ac5c:      add x24, x8, x22, lsl #3
10089ac60:      str x23, [x24]
10089ac64:      mov x0, x21
10089ac68:      bl  0x1008ddb44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10089ac6c:      tbz w0, #0x0, 0x10089aca8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6a8>
10089ac70:      cmp x28, x23, lsr #48
10089ac74:      b.ne    0x10089acc8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6c8>
10089ac78:      mov x0, x23
10089ac7c:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089ac80:      tbnz    w0, #0x0, 0x10089ace4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10089ac84:      scvtf   d0, w23
10089ac88:      b   0x10089ace0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e0>
10089ac8c:      fmov    d0, x23
10089ac90:      mov x0, x21
10089ac94:      bl  0x100848268 <_js_array_push_f64>
10089ac98:      add x25, x25, #0x8
10089ac9c:      cmp x25, #0x40
10089aca0:      b.ne    0x10089ac48 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10089aca4:      b   0x10089adc4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10089aca8:      cmp x26, #0x201
10089acac:      b.lo    0x10089ace4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10089acb0:      ldurb   w8, [x21, #-0x8]
10089acb4:      cmp w8, #0x1
10089acb8:      b.ne    0x10089ace4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10089acbc:      ldurh   w8, [x21, #-0x6]
10089acc0:      tbnz    w8, #0xc, 0x10089ac70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x670>
10089acc4:      b   0x10089ace4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10089acc8:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10089accc:      cmp x23, x8
10089acd0:      b.gt    0x10089ace4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10089acd4:      fmov    d0, x23
10089acd8:      fcmp    d0, d0
10089acdc:      fcsel   d0, d8, d0, vs
10089ace0:      fmov    x23, d0
10089ace4:      str x23, [x24]
10089ace8:      cmp x28, x23, lsr #48
10089acec:      b.ne    0x10089ad04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x704>
10089acf0:      mov x0, x23
10089acf4:      bl  0x10024367c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10089acf8:      tbnz    w0, #0x0, 0x10089ad10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x710>
10089acfc:      scvtf   d0, w23
10089ad00:      b   0x10089ad58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x758>
10089ad04:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10089ad08:      cmp x23, x8
10089ad0c:      b.le    0x10089ad4c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x74c>
10089ad10:      cmp x26, #0x201
10089ad14:      b.lo    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad18:      ldurb   w8, [x21, #-0x8]
10089ad1c:      cmp w8, #0x1
10089ad20:      b.ne    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad24:      ldurh   w8, [x21, #-0x6]
10089ad28:      mov w9, #0xef7f             ; =61311
10089ad2c:      and w9, w8, w9
10089ad30:      sturh   w9, [x21, #-0x6]
10089ad34:      mov w9, #0x1080             ; =4224
10089ad38:      tst w8, w9
10089ad3c:      b.eq    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad40:      mov x0, x21
10089ad44:      bl  0x1007bd668 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10089ad48:      b   0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad4c:      fmov    d0, x23
10089ad50:      fcmp    d0, d0
10089ad54:      fcsel   d0, d8, d0, vs
10089ad58:      cmp x26, #0x201
10089ad5c:      b.lo    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad60:      ldurb   w8, [x21, #-0x8]
10089ad64:      cmp w8, #0x1
10089ad68:      b.ne    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad6c:      ldurh   w8, [x21, #-0x6]
10089ad70:      tbz w8, #0x7, 0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad74:      ldr w8, [x21]
10089ad78:      cmp w22, w8
10089ad7c:      b.hs    0x10089ad84 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10089ad80:      str d0, [x24]
10089ad84:      mov x0, x21
10089ad88:      mov x1, x22
10089ad8c:      mov x2, x23
10089ad90:      bl  0x1007ea6c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10089ad94:      mov x0, x21
10089ad98:      bl  0x1008cc038 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10089ad9c:      cbz w0, 0x10089ac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10089ada0:      mov x0, x21
10089ada4:      mov x1, x24
10089ada8:      mov x2, x23
10089adac:      bl  0x10054f298 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc13barrier_store26runtime_write_barrier_slot>
10089adb0:      b   0x10089ac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10089adb4:      mov w1, #0x8                ; =8
10089adb8:      b   0x10089a898 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10089adbc:      mov w1, #0x8                ; =8
10089adc0:      b   0x10089a7d0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10089adc4:      mov x0, x20
10089adc8:      mov x1, x21
10089adcc:      mov x2, x19
10089add0:      ldp x29, x30, [sp, #0xa0]
10089add4:      ldp x20, x19, [sp, #0x90]
10089add8:      ldp x22, x21, [sp, #0x80]
10089addc:      ldp x24, x23, [sp, #0x70]
10089ade0:      ldp x26, x25, [sp, #0x60]
10089ade4:      ldp x28, x27, [sp, #0x50]
10089ade8:      ldp d9, d8, [sp, #0x40]
10089adec:      add sp, sp, #0xb0
10089adf0:      b   0x100899df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
