/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010028b724 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>:
10028b724:      sub sp, sp, #0xb0
10028b728:      stp d9, d8, [sp, #0x40]
10028b72c:      stp x28, x27, [sp, #0x50]
10028b730:      stp x26, x25, [sp, #0x60]
10028b734:      stp x24, x23, [sp, #0x70]
10028b738:      stp x22, x21, [sp, #0x80]
10028b73c:      stp x20, x19, [sp, #0x90]
10028b740:      stp x29, x30, [sp, #0xa0]
10028b744:      add x29, sp, #0xa0
10028b748:      mov x19, x1
10028b74c:      mov x20, x0
10028b750:      adrp    x1, 0x100dc2000 <_anon.1be6a8e5f67bdf40065326454fc916e9.205+0x47>
10028b754:      add x1, x1, #0xe80
10028b758:      mov x0, sp
10028b75c:      mov w2, #0x40               ; =64
10028b760:      bl  0x100ce4410 <_writev+0x100ce4410>
10028b764:      ldp x9, x8, [x20, #0x30]
10028b768:      cmp x8, x9
10028b76c:      b.hs    0x10028b7d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10028b770:      ldr x9, [x20, #0x28]
10028b774:      ldrb    w9, [x9, x8]
10028b778:      cmp w9, #0x5d
10028b77c:      b.ne    0x10028b7d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
10028b780:      add x8, x8, #0x1
10028b784:      str x8, [x20, #0x38]
10028b788:      mov w0, #0x0                ; =0
10028b78c:      bl  0x1002cd8c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
10028b790:      mov x20, x0
10028b794:      bl  0x1002cf378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
10028b798:      adrp    x0, 0x101136000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime5error21CURRENT_CALL_LOCATION0s_023___RUST_STD_INTERNAL_VAL+0x10>
10028b79c:      add x0, x0, #0x590
10028b7a0:      ldr x8, [x0]
10028b7a4:      blr x8
10028b7a8:      ldrb    w8, [x0, #0x20]
10028b7ac:      cbnz    w8, 0x10028bb08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3e4>
10028b7b0:      ldr x8, [x0]
10028b7b4:      cbnz    x8, 0x10028bb30 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x40c>
10028b7b8:      ldr x8, [x0, #0x18]
10028b7bc:      cmp x19, x8
10028b7c0:      b.hi    0x10028b7c8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xa4>
10028b7c4:      str x19, [x0, #0x18]
10028b7c8:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
10028b7cc:      bfxil   x0, x20, #0, #48
10028b7d0:      b   0x10028b9cc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2a8>
10028b7d4:      mov x0, x20
10028b7d8:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028b7dc:      ldrb    w8, [x20, #0x90]
10028b7e0:      tbz w8, #0x0, 0x10028b82c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x108>
10028b7e4:      str x0, [sp]
10028b7e8:      ldp x9, x8, [x20, #0x30]
10028b7ec:      cmp x8, x9
10028b7f0:      b.hs    0x10028b870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10028b7f4:      ldr x10, [x20, #0x28]
10028b7f8:      mov x11, #0x2600            ; =9728
10028b7fc:      movk    x11, #0x1, lsl #32
10028b800:      mov w1, #0x1                ; =1
10028b804:      ldrb    w12, [x10, x8]
10028b808:      cmp w12, #0x20
10028b80c:      b.hi    0x10028b870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10028b810:      lsr x12, x11, x12
10028b814:      tbz w12, #0x0, 0x10028b870 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
10028b818:      add x8, x8, #0x1
10028b81c:      str x8, [x20, #0x38]
10028b820:      cmp x9, x8
10028b824:      b.ne    0x10028b804 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xe0>
10028b828:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b82c:      mov x1, #0x0                ; =0
10028b830:      ldp x9, x8, [x20, #0x30]
10028b834:      cmp x8, x9
10028b838:      b.hs    0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b83c:      ldr x10, [x20, #0x28]
10028b840:      mov x11, #0x2600            ; =9728
10028b844:      movk    x11, #0x1, lsl #32
10028b848:      ldrb    w12, [x10, x8]
10028b84c:      cmp w12, #0x20
10028b850:      b.hi    0x10028b924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10028b854:      lsr x13, x11, x12
10028b858:      tbz w13, #0x0, 0x10028b924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10028b85c:      add x8, x8, #0x1
10028b860:      str x8, [x20, #0x38]
10028b864:      cmp x9, x8
10028b868:      b.ne    0x10028b848 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x124>
10028b86c:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b870:      cmp x8, x9
10028b874:      b.hs    0x10028b8e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1c4>
10028b878:      ldr x10, [x20, #0x28]
10028b87c:      ldrb    w11, [x10, x8]
10028b880:      cmp w11, #0x2c
10028b884:      b.ne    0x10028b8f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1cc>
10028b888:      add x8, x8, #0x1
10028b88c:      str x8, [x20, #0x38]
10028b890:      mov x0, x20
10028b894:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028b898:      ldrb    w8, [x20, #0x90]
10028b89c:      cbz w8, 0x10028b938 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x214>
10028b8a0:      str x0, [sp, #0x8]
10028b8a4:      ldp x9, x8, [x20, #0x30]
10028b8a8:      cmp x8, x9
10028b8ac:      b.hs    0x10028b940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10028b8b0:      ldr x10, [x20, #0x28]
10028b8b4:      mov x11, #0x2600            ; =9728
10028b8b8:      movk    x11, #0x1, lsl #32
10028b8bc:      mov w1, #0x2                ; =2
10028b8c0:      ldrb    w12, [x10, x8]
10028b8c4:      cmp w12, #0x20
10028b8c8:      b.hi    0x10028b940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10028b8cc:      lsr x12, x11, x12
10028b8d0:      tbz w12, #0x0, 0x10028b940 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
10028b8d4:      add x8, x8, #0x1
10028b8d8:      str x8, [x20, #0x38]
10028b8dc:      cmp x9, x8
10028b8e0:      b.ne    0x10028b8c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x19c>
10028b8e4:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b8e8:      mov w1, #0x1                ; =1
10028b8ec:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b8f0:      mov w1, #0x1                ; =1
10028b8f4:      mov x11, #0x2600            ; =9728
10028b8f8:      movk    x11, #0x1, lsl #32
10028b8fc:      ldrb    w12, [x10, x8]
10028b900:      cmp w12, #0x20
10028b904:      b.hi    0x10028b924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10028b908:      lsr x13, x11, x12
10028b90c:      tbz w13, #0x0, 0x10028b924 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
10028b910:      add x8, x8, #0x1
10028b914:      str x8, [x20, #0x38]
10028b918:      cmp x9, x8
10028b91c:      b.ne    0x10028b8fc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d8>
10028b920:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b924:      cmp w12, #0x5d
10028b928:      b.ne    0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b92c:      add x8, x8, #0x1
10028b930:      str x8, [x20, #0x38]
10028b934:      b   0x10028b9c0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x29c>
10028b938:      mov w1, #0x1                ; =1
10028b93c:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028b940:      cmp x8, x9
10028b944:      b.hs    0x10028b9b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x294>
10028b948:      ldr x10, [x20, #0x28]
10028b94c:      ldrb    w11, [x10, x8]
10028b950:      cmp w11, #0x2c
10028b954:      b.ne    0x10028b9f0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2cc>
10028b958:      add x8, x8, #0x1
10028b95c:      str x8, [x20, #0x38]
10028b960:      mov x0, x20
10028b964:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028b968:      ldrb    w8, [x20, #0x90]
10028b96c:      cbz w8, 0x10028b9f8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2d4>
10028b970:      str x0, [sp, #0x10]
10028b974:      ldp x9, x8, [x20, #0x30]
10028b978:      cmp x8, x9
10028b97c:      b.hs    0x10028ba00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10028b980:      ldr x10, [x20, #0x28]
10028b984:      mov x11, #0x2600            ; =9728
10028b988:      movk    x11, #0x1, lsl #32
10028b98c:      mov w1, #0x3                ; =3
10028b990:      ldrb    w12, [x10, x8]
10028b994:      cmp w12, #0x20
10028b998:      b.hi    0x10028ba00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10028b99c:      lsr x12, x11, x12
10028b9a0:      tbz w12, #0x0, 0x10028ba00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
10028b9a4:      add x8, x8, #0x1
10028b9a8:      str x8, [x20, #0x38]
10028b9ac:      cmp x9, x8
10028b9b0:      b.ne    0x10028b990 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x26c>
10028b9b4:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028b9b8:      mov w1, #0x2                ; =2
10028b9bc:      strb    wzr, [x20, #0x90]
10028b9c0:      mov x0, sp
10028b9c4:      mov x2, x19
10028b9c8:      bl  0x10028b474 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>
10028b9cc:      ldp x29, x30, [sp, #0xa0]
10028b9d0:      ldp x20, x19, [sp, #0x90]
10028b9d4:      ldp x22, x21, [sp, #0x80]
10028b9d8:      ldp x24, x23, [sp, #0x70]
10028b9dc:      ldp x26, x25, [sp, #0x60]
10028b9e0:      ldp x28, x27, [sp, #0x50]
10028b9e4:      ldp d9, d8, [sp, #0x40]
10028b9e8:      add sp, sp, #0xb0
10028b9ec:      ret
10028b9f0:      mov w1, #0x2                ; =2
10028b9f4:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028b9f8:      mov w1, #0x2                ; =2
10028b9fc:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028ba00:      cmp x8, x9
10028ba04:      b.hs    0x10028ba78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x354>
10028ba08:      ldr x10, [x20, #0x28]
10028ba0c:      ldrb    w11, [x10, x8]
10028ba10:      cmp w11, #0x2c
10028ba14:      b.ne    0x10028ba80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x35c>
10028ba18:      add x8, x8, #0x1
10028ba1c:      str x8, [x20, #0x38]
10028ba20:      mov x0, x20
10028ba24:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028ba28:      ldrb    w8, [x20, #0x90]
10028ba2c:      cbz w8, 0x10028ba88 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x364>
10028ba30:      str x0, [sp, #0x18]
10028ba34:      ldp x9, x8, [x20, #0x30]
10028ba38:      cmp x8, x9
10028ba3c:      b.hs    0x10028ba90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10028ba40:      ldr x10, [x20, #0x28]
10028ba44:      mov x11, #0x2600            ; =9728
10028ba48:      movk    x11, #0x1, lsl #32
10028ba4c:      mov w1, #0x4                ; =4
10028ba50:      ldrb    w12, [x10, x8]
10028ba54:      cmp w12, #0x20
10028ba58:      b.hi    0x10028ba90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10028ba5c:      lsr x12, x11, x12
10028ba60:      tbz w12, #0x0, 0x10028ba90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
10028ba64:      add x8, x8, #0x1
10028ba68:      str x8, [x20, #0x38]
10028ba6c:      cmp x9, x8
10028ba70:      b.ne    0x10028ba50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x32c>
10028ba74:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028ba78:      mov w1, #0x3                ; =3
10028ba7c:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028ba80:      mov w1, #0x3                ; =3
10028ba84:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028ba88:      mov w1, #0x3                ; =3
10028ba8c:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028ba90:      cmp x8, x9
10028ba94:      b.hs    0x10028bb3c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x418>
10028ba98:      ldr x10, [x20, #0x28]
10028ba9c:      ldrb    w11, [x10, x8]
10028baa0:      cmp w11, #0x2c
10028baa4:      b.ne    0x10028bb50 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x42c>
10028baa8:      add x8, x8, #0x1
10028baac:      str x8, [x20, #0x38]
10028bab0:      mov x0, x20
10028bab4:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028bab8:      ldrb    w8, [x20, #0x90]
10028babc:      cbz w8, 0x10028bb58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x434>
10028bac0:      str x0, [sp, #0x20]
10028bac4:      ldp x9, x8, [x20, #0x30]
10028bac8:      cmp x8, x9
10028bacc:      b.hs    0x10028bb60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10028bad0:      ldr x10, [x20, #0x28]
10028bad4:      mov x11, #0x2600            ; =9728
10028bad8:      movk    x11, #0x1, lsl #32
10028badc:      mov w1, #0x5                ; =5
10028bae0:      ldrb    w12, [x10, x8]
10028bae4:      cmp w12, #0x20
10028bae8:      b.hi    0x10028bb60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10028baec:      lsr x12, x11, x12
10028baf0:      tbz w12, #0x0, 0x10028bb60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
10028baf4:      add x8, x8, #0x1
10028baf8:      str x8, [x20, #0x38]
10028bafc:      cmp x9, x8
10028bb00:      b.ne    0x10028bae0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3bc>
10028bb04:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bb08:      cmp w8, #0x1
10028bb0c:      b.ne    0x10028bb44 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x420>
10028bb10:      adrp    x1, 0x100820000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot7HotCellINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1Y_6option6OptionINtNtNtNtBa_11collections4hash3map7HashMapmNtNtB1a_4yoga8YogaNodeEEEKj1_EEB1a_+0xe8>
10028bb14:      add x1, x1, #0xf78
10028bb18:      mov x21, x0
10028bb1c:      bl  0x100ba67dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10028bb20:      mov x0, x21
10028bb24:      strb    wzr, [x21, #0x20]
10028bb28:      ldr x8, [x21]
10028bb2c:      cbz x8, 0x10028b7b8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x94>
10028bb30:      adrp    x0, 0x1010a0000 <_anon.58120679d426c7dccd15bda76f596bde.21>
10028bb34:      add x0, x0, #0xe58
10028bb38:      bl  0x100c9852c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10028bb3c:      mov w1, #0x4                ; =4
10028bb40:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bb44:      adrp    x0, 0x10109f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10028bb48:      add x0, x0, #0xed8
10028bb4c:      bl  0x100cdab9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10028bb50:      mov w1, #0x4                ; =4
10028bb54:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028bb58:      mov w1, #0x4                ; =4
10028bb5c:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028bb60:      cmp x8, x9
10028bb64:      b.hs    0x10028bbd8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4b4>
10028bb68:      ldr x10, [x20, #0x28]
10028bb6c:      ldrb    w11, [x10, x8]
10028bb70:      cmp w11, #0x2c
10028bb74:      b.ne    0x10028bbe0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4bc>
10028bb78:      add x8, x8, #0x1
10028bb7c:      str x8, [x20, #0x38]
10028bb80:      mov x0, x20
10028bb84:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028bb88:      ldrb    w8, [x20, #0x90]
10028bb8c:      cbz w8, 0x10028bbe8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4c4>
10028bb90:      str x0, [sp, #0x28]
10028bb94:      ldp x9, x8, [x20, #0x30]
10028bb98:      cmp x8, x9
10028bb9c:      b.hs    0x10028bbf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10028bba0:      ldr x10, [x20, #0x28]
10028bba4:      mov x11, #0x2600            ; =9728
10028bba8:      movk    x11, #0x1, lsl #32
10028bbac:      mov w1, #0x6                ; =6
10028bbb0:      ldrb    w12, [x10, x8]
10028bbb4:      cmp w12, #0x20
10028bbb8:      b.hi    0x10028bbf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10028bbbc:      lsr x12, x11, x12
10028bbc0:      tbz w12, #0x0, 0x10028bbf0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
10028bbc4:      add x8, x8, #0x1
10028bbc8:      str x8, [x20, #0x38]
10028bbcc:      cmp x9, x8
10028bbd0:      b.ne    0x10028bbb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x48c>
10028bbd4:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bbd8:      mov w1, #0x5                ; =5
10028bbdc:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bbe0:      mov w1, #0x5                ; =5
10028bbe4:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028bbe8:      mov w1, #0x5                ; =5
10028bbec:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028bbf0:      cmp x8, x9
10028bbf4:      b.hs    0x10028bc68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x544>
10028bbf8:      ldr x10, [x20, #0x28]
10028bbfc:      ldrb    w11, [x10, x8]
10028bc00:      cmp w11, #0x2c
10028bc04:      b.ne    0x10028bc70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x54c>
10028bc08:      add x8, x8, #0x1
10028bc0c:      str x8, [x20, #0x38]
10028bc10:      mov x0, x20
10028bc14:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028bc18:      ldrb    w8, [x20, #0x90]
10028bc1c:      cbz w8, 0x10028bc78 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x554>
10028bc20:      str x0, [sp, #0x30]
10028bc24:      ldp x9, x8, [x20, #0x30]
10028bc28:      cmp x8, x9
10028bc2c:      b.hs    0x10028bc80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10028bc30:      ldr x10, [x20, #0x28]
10028bc34:      mov x11, #0x2600            ; =9728
10028bc38:      movk    x11, #0x1, lsl #32
10028bc3c:      mov w1, #0x7                ; =7
10028bc40:      ldrb    w12, [x10, x8]
10028bc44:      cmp w12, #0x20
10028bc48:      b.hi    0x10028bc80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10028bc4c:      lsr x12, x11, x12
10028bc50:      tbz w12, #0x0, 0x10028bc80 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
10028bc54:      add x8, x8, #0x1
10028bc58:      str x8, [x20, #0x38]
10028bc5c:      cmp x9, x8
10028bc60:      b.ne    0x10028bc40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x51c>
10028bc64:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bc68:      mov w1, #0x6                ; =6
10028bc6c:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bc70:      mov w1, #0x6                ; =6
10028bc74:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028bc78:      mov w1, #0x6                ; =6
10028bc7c:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028bc80:      cmp x8, x9
10028bc84:      b.hs    0x10028bcf8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5d4>
10028bc88:      ldr x10, [x20, #0x28]
10028bc8c:      ldrb    w11, [x10, x8]
10028bc90:      cmp w11, #0x2c
10028bc94:      b.ne    0x10028bd00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5dc>
10028bc98:      add x8, x8, #0x1
10028bc9c:      str x8, [x20, #0x38]
10028bca0:      mov x0, x20
10028bca4:      bl  0x10028a9a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
10028bca8:      ldrb    w8, [x20, #0x90]
10028bcac:      cbz w8, 0x10028bd08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5e4>
10028bcb0:      str x0, [sp, #0x38]
10028bcb4:      ldp x9, x8, [x20, #0x30]
10028bcb8:      cmp x8, x9
10028bcbc:      b.hs    0x10028bd10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10028bcc0:      ldr x10, [x20, #0x28]
10028bcc4:      mov x11, #0x2600            ; =9728
10028bcc8:      movk    x11, #0x1, lsl #32
10028bccc:      mov w1, #0x8                ; =8
10028bcd0:      ldrb    w12, [x10, x8]
10028bcd4:      cmp w12, #0x20
10028bcd8:      b.hi    0x10028bd10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10028bcdc:      lsr x12, x11, x12
10028bce0:      tbz w12, #0x0, 0x10028bd10 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
10028bce4:      add x8, x8, #0x1
10028bce8:      str x8, [x20, #0x38]
10028bcec:      cmp x9, x8
10028bcf0:      b.ne    0x10028bcd0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ac>
10028bcf4:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bcf8:      mov w1, #0x7                ; =7
10028bcfc:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bd00:      mov w1, #0x7                ; =7
10028bd04:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028bd08:      mov w1, #0x7                ; =7
10028bd0c:      b   0x10028b830 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
10028bd10:      cmp x8, x9
10028bd14:      b.hs    0x10028bed8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7b4>
10028bd18:      ldr x10, [x20, #0x28]
10028bd1c:      ldrb    w11, [x10, x8]
10028bd20:      cmp w11, #0x2c
10028bd24:      b.ne    0x10028bee0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7bc>
10028bd28:      add x8, x8, #0x1
10028bd2c:      str x8, [x20, #0x38]
10028bd30:      mov w0, #0x10               ; =16
10028bd34:      bl  0x1002f710c <_js_array_alloc>
10028bd38:      mov x21, x0
10028bd3c:      mov x25, #0x0               ; =0
10028bd40:      mov x27, sp
10028bd44:      mov w28, #0x7ffe            ; =32766
10028bd48:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
10028bd4c:      fmov    d8, x8
10028bd50:      lsr x26, x0, #3
10028bd54:      b   0x10028bd6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10028bd58:      add w8, w22, #0x1
10028bd5c:      str w8, [x21]
10028bd60:      add x25, x25, #0x8
10028bd64:      cmp x25, #0x40
10028bd68:      b.eq    0x10028bee8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10028bd6c:      ldr x23, [x27, x25]
10028bd70:      ldp w22, w8, [x21]
10028bd74:      cmp w22, w8
10028bd78:      b.hs    0x10028bdb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x68c>
10028bd7c:      add x8, x21, #0x8
10028bd80:      add x24, x8, x22, lsl #3
10028bd84:      str x23, [x24]
10028bd88:      mov x0, x21
10028bd8c:      bl  0x1002ce484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
10028bd90:      tbz w0, #0x0, 0x10028bdcc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6a8>
10028bd94:      cmp x28, x23, lsr #48
10028bd98:      b.ne    0x10028bdec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6c8>
10028bd9c:      mov x0, x23
10028bda0:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028bda4:      tbnz    w0, #0x0, 0x10028be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10028bda8:      scvtf   d0, w23
10028bdac:      b   0x10028be04 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e0>
10028bdb0:      fmov    d0, x23
10028bdb4:      mov x0, x21
10028bdb8:      bl  0x100493508 <_js_array_push_f64>
10028bdbc:      add x25, x25, #0x8
10028bdc0:      cmp x25, #0x40
10028bdc4:      b.ne    0x10028bd6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
10028bdc8:      b   0x10028bee8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
10028bdcc:      cmp x26, #0x201
10028bdd0:      b.lo    0x10028be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10028bdd4:      ldurb   w8, [x21, #-0x8]
10028bdd8:      cmp w8, #0x1
10028bddc:      b.ne    0x10028be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10028bde0:      ldurh   w8, [x21, #-0x6]
10028bde4:      tbnz    w8, #0xc, 0x10028bd94 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x670>
10028bde8:      b   0x10028be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10028bdec:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10028bdf0:      cmp x23, x8
10028bdf4:      b.gt    0x10028be08 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
10028bdf8:      fmov    d0, x23
10028bdfc:      fcmp    d0, d0
10028be00:      fcsel   d0, d8, d0, vs
10028be04:      fmov    x23, d0
10028be08:      str x23, [x24]
10028be0c:      cmp x28, x23, lsr #48
10028be10:      b.ne    0x10028be28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x704>
10028be14:      mov x0, x23
10028be18:      bl  0x100aa65f8 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
10028be1c:      tbnz    w0, #0x0, 0x10028be34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x710>
10028be20:      scvtf   d0, w23
10028be24:      b   0x10028be7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x758>
10028be28:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
10028be2c:      cmp x23, x8
10028be30:      b.le    0x10028be70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x74c>
10028be34:      cmp x26, #0x201
10028be38:      b.lo    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be3c:      ldurb   w8, [x21, #-0x8]
10028be40:      cmp w8, #0x1
10028be44:      b.ne    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be48:      ldurh   w8, [x21, #-0x6]
10028be4c:      mov w9, #0xef7f             ; =61311
10028be50:      and w9, w8, w9
10028be54:      sturh   w9, [x21, #-0x6]
10028be58:      mov w9, #0x1080             ; =4224
10028be5c:      tst w8, w9
10028be60:      b.eq    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be64:      mov x0, x21
10028be68:      bl  0x100422620 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
10028be6c:      b   0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be70:      fmov    d0, x23
10028be74:      fcmp    d0, d0
10028be78:      fcsel   d0, d8, d0, vs
10028be7c:      cmp x26, #0x201
10028be80:      b.lo    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be84:      ldurb   w8, [x21, #-0x8]
10028be88:      cmp w8, #0x1
10028be8c:      b.ne    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be90:      ldurh   w8, [x21, #-0x6]
10028be94:      tbz w8, #0x7, 0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028be98:      ldr w8, [x21]
10028be9c:      cmp w22, w8
10028bea0:      b.hs    0x10028bea8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
10028bea4:      str d0, [x24]
10028bea8:      mov x0, x21
10028beac:      mov x1, x22
10028beb0:      mov x2, x23
10028beb4:      bl  0x1001d7e00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
10028beb8:      mov x0, x21
10028bebc:      bl  0x1002c0a1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
10028bec0:      cbz w0, 0x10028bd58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10028bec4:      mov x0, x21
10028bec8:      mov x1, x24
10028becc:      mov x2, x23
10028bed0:      bl  0x100433584 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc13barrier_store26runtime_write_barrier_slot>
10028bed4:      b   0x10028bd58 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
10028bed8:      mov w1, #0x8                ; =8
10028bedc:      b   0x10028b9bc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
10028bee0:      mov w1, #0x8                ; =8
10028bee4:      b   0x10028b8f4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
10028bee8:      mov x0, x20
10028beec:      mov x1, x21
10028bef0:      mov x2, x19
10028bef4:      ldp x29, x30, [sp, #0xa0]
10028bef8:      ldp x20, x19, [sp, #0x90]
10028befc:      ldp x22, x21, [sp, #0x80]
10028bf00:      ldp x24, x23, [sp, #0x70]
10028bf04:      ldp x26, x25, [sp, #0x60]
10028bf08:      ldp x28, x27, [sp, #0x50]
10028bf0c:      ldp d9, d8, [sp, #0x40]
10028bf10:      add sp, sp, #0xb0
10028bf14:      b   0x10028af18 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
