/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003bab90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>:
1003bab90:      sub sp, sp, #0xb0
1003bab94:      stp d9, d8, [sp, #0x40]
1003bab98:      stp x28, x27, [sp, #0x50]
1003bab9c:      stp x26, x25, [sp, #0x60]
1003baba0:      stp x24, x23, [sp, #0x70]
1003baba4:      stp x22, x21, [sp, #0x80]
1003baba8:      stp x20, x19, [sp, #0x90]
1003babac:      stp x29, x30, [sp, #0xa0]
1003babb0:      add x29, sp, #0xa0
1003babb4:      mov x19, x1
1003babb8:      mov x20, x0
1003babbc:      adrp    x1, 0x100dbd000 <_anon.4bdeadb2bc5806cd6237d477ded05be6.1392+0xd>
1003babc0:      add x1, x1, #0x8a0
1003babc4:      mov x0, sp
1003babc8:      mov w2, #0x40               ; =64
1003babcc:      bl  0x100cd8dd0 <_writev+0x100cd8dd0>
1003babd0:      ldp x9, x8, [x20, #0x30]
1003babd4:      cmp x8, x9
1003babd8:      b.hs    0x1003bac40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
1003babdc:      ldr x9, [x20, #0x28]
1003babe0:      ldrb    w9, [x9, x8]
1003babe4:      cmp w9, #0x5d
1003babe8:      b.ne    0x1003bac40 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xb0>
1003babec:      add x8, x8, #0x1
1003babf0:      str x8, [x20, #0x38]
1003babf4:      mov w0, #0x0                ; =0
1003babf8:      bl  0x1003fb814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array5alloc32js_array_alloc_with_length_exact>
1003babfc:      mov x20, x0
1003bac00:      bl  0x1003fd2b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header24set_array_numeric_layout>
1003bac04:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003bac08:      add x0, x0, #0x8
1003bac0c:      ldr x8, [x0]
1003bac10:      blr x8
1003bac14:      ldrb    w8, [x0, #0x20]
1003bac18:      cbnz    w8, 0x1003baf74 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3e4>
1003bac1c:      ldr x8, [x0]
1003bac20:      cbnz    x8, 0x1003baf9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x40c>
1003bac24:      ldr x8, [x0, #0x18]
1003bac28:      cmp x19, x8
1003bac2c:      b.hi    0x1003bac34 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xa4>
1003bac30:      str x19, [x0, #0x18]
1003bac34:      mov x0, #0x7ffd000000000000 ; =9222527611924643840
1003bac38:      bfxil   x0, x20, #0, #48
1003bac3c:      b   0x1003bae38 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2a8>
1003bac40:      mov x0, x20
1003bac44:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003bac48:      ldrb    w8, [x20, #0x90]
1003bac4c:      tbz w8, #0x0, 0x1003bac98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x108>
1003bac50:      str x0, [sp]
1003bac54:      ldp x9, x8, [x20, #0x30]
1003bac58:      cmp x8, x9
1003bac5c:      b.hs    0x1003bacdc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
1003bac60:      ldr x10, [x20, #0x28]
1003bac64:      mov x11, #0x2600            ; =9728
1003bac68:      movk    x11, #0x1, lsl #32
1003bac6c:      mov w1, #0x1                ; =1
1003bac70:      ldrb    w12, [x10, x8]
1003bac74:      cmp w12, #0x20
1003bac78:      b.hi    0x1003bacdc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
1003bac7c:      lsr x12, x11, x12
1003bac80:      tbz w12, #0x0, 0x1003bacdc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x14c>
1003bac84:      add x8, x8, #0x1
1003bac88:      str x8, [x20, #0x38]
1003bac8c:      cmp x9, x8
1003bac90:      b.ne    0x1003bac70 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0xe0>
1003bac94:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bac98:      mov x1, #0x0                ; =0
1003bac9c:      ldp x9, x8, [x20, #0x30]
1003baca0:      cmp x8, x9
1003baca4:      b.hs    0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003baca8:      ldr x10, [x20, #0x28]
1003bacac:      mov x11, #0x2600            ; =9728
1003bacb0:      movk    x11, #0x1, lsl #32
1003bacb4:      ldrb    w12, [x10, x8]
1003bacb8:      cmp w12, #0x20
1003bacbc:      b.hi    0x1003bad90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
1003bacc0:      lsr x13, x11, x12
1003bacc4:      tbz w13, #0x0, 0x1003bad90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
1003bacc8:      add x8, x8, #0x1
1003baccc:      str x8, [x20, #0x38]
1003bacd0:      cmp x9, x8
1003bacd4:      b.ne    0x1003bacb4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x124>
1003bacd8:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bacdc:      cmp x8, x9
1003bace0:      b.hs    0x1003bad54 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1c4>
1003bace4:      ldr x10, [x20, #0x28]
1003bace8:      ldrb    w11, [x10, x8]
1003bacec:      cmp w11, #0x2c
1003bacf0:      b.ne    0x1003bad5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1cc>
1003bacf4:      add x8, x8, #0x1
1003bacf8:      str x8, [x20, #0x38]
1003bacfc:      mov x0, x20
1003bad00:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003bad04:      ldrb    w8, [x20, #0x90]
1003bad08:      cbz w8, 0x1003bada4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x214>
1003bad0c:      str x0, [sp, #0x8]
1003bad10:      ldp x9, x8, [x20, #0x30]
1003bad14:      cmp x8, x9
1003bad18:      b.hs    0x1003badac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1003bad1c:      ldr x10, [x20, #0x28]
1003bad20:      mov x11, #0x2600            ; =9728
1003bad24:      movk    x11, #0x1, lsl #32
1003bad28:      mov w1, #0x2                ; =2
1003bad2c:      ldrb    w12, [x10, x8]
1003bad30:      cmp w12, #0x20
1003bad34:      b.hi    0x1003badac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1003bad38:      lsr x12, x11, x12
1003bad3c:      tbz w12, #0x0, 0x1003badac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x21c>
1003bad40:      add x8, x8, #0x1
1003bad44:      str x8, [x20, #0x38]
1003bad48:      cmp x9, x8
1003bad4c:      b.ne    0x1003bad2c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x19c>
1003bad50:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bad54:      mov w1, #0x1                ; =1
1003bad58:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bad5c:      mov w1, #0x1                ; =1
1003bad60:      mov x11, #0x2600            ; =9728
1003bad64:      movk    x11, #0x1, lsl #32
1003bad68:      ldrb    w12, [x10, x8]
1003bad6c:      cmp w12, #0x20
1003bad70:      b.hi    0x1003bad90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
1003bad74:      lsr x13, x11, x12
1003bad78:      tbz w13, #0x0, 0x1003bad90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x200>
1003bad7c:      add x8, x8, #0x1
1003bad80:      str x8, [x20, #0x38]
1003bad84:      cmp x9, x8
1003bad88:      b.ne    0x1003bad68 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d8>
1003bad8c:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bad90:      cmp w12, #0x5d
1003bad94:      b.ne    0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bad98:      add x8, x8, #0x1
1003bad9c:      str x8, [x20, #0x38]
1003bada0:      b   0x1003bae2c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x29c>
1003bada4:      mov w1, #0x1                ; =1
1003bada8:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003badac:      cmp x8, x9
1003badb0:      b.hs    0x1003bae24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x294>
1003badb4:      ldr x10, [x20, #0x28]
1003badb8:      ldrb    w11, [x10, x8]
1003badbc:      cmp w11, #0x2c
1003badc0:      b.ne    0x1003bae5c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2cc>
1003badc4:      add x8, x8, #0x1
1003badc8:      str x8, [x20, #0x38]
1003badcc:      mov x0, x20
1003badd0:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003badd4:      ldrb    w8, [x20, #0x90]
1003badd8:      cbz w8, 0x1003bae64 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2d4>
1003baddc:      str x0, [sp, #0x10]
1003bade0:      ldp x9, x8, [x20, #0x30]
1003bade4:      cmp x8, x9
1003bade8:      b.hs    0x1003bae6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
1003badec:      ldr x10, [x20, #0x28]
1003badf0:      mov x11, #0x2600            ; =9728
1003badf4:      movk    x11, #0x1, lsl #32
1003badf8:      mov w1, #0x3                ; =3
1003badfc:      ldrb    w12, [x10, x8]
1003bae00:      cmp w12, #0x20
1003bae04:      b.hi    0x1003bae6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
1003bae08:      lsr x12, x11, x12
1003bae0c:      tbz w12, #0x0, 0x1003bae6c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x2dc>
1003bae10:      add x8, x8, #0x1
1003bae14:      str x8, [x20, #0x38]
1003bae18:      cmp x9, x8
1003bae1c:      b.ne    0x1003badfc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x26c>
1003bae20:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bae24:      mov w1, #0x2                ; =2
1003bae28:      strb    wzr, [x20, #0x90]
1003bae2c:      mov x0, sp
1003bae30:      mov x2, x19
1003bae34:      bl  0x1003ba8e0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18finish_short_array>
1003bae38:      ldp x29, x30, [sp, #0xa0]
1003bae3c:      ldp x20, x19, [sp, #0x90]
1003bae40:      ldp x22, x21, [sp, #0x80]
1003bae44:      ldp x24, x23, [sp, #0x70]
1003bae48:      ldp x26, x25, [sp, #0x60]
1003bae4c:      ldp x28, x27, [sp, #0x50]
1003bae50:      ldp d9, d8, [sp, #0x40]
1003bae54:      add sp, sp, #0xb0
1003bae58:      ret
1003bae5c:      mov w1, #0x2                ; =2
1003bae60:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bae64:      mov w1, #0x2                ; =2
1003bae68:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003bae6c:      cmp x8, x9
1003bae70:      b.hs    0x1003baee4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x354>
1003bae74:      ldr x10, [x20, #0x28]
1003bae78:      ldrb    w11, [x10, x8]
1003bae7c:      cmp w11, #0x2c
1003bae80:      b.ne    0x1003baeec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x35c>
1003bae84:      add x8, x8, #0x1
1003bae88:      str x8, [x20, #0x38]
1003bae8c:      mov x0, x20
1003bae90:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003bae94:      ldrb    w8, [x20, #0x90]
1003bae98:      cbz w8, 0x1003baef4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x364>
1003bae9c:      str x0, [sp, #0x18]
1003baea0:      ldp x9, x8, [x20, #0x30]
1003baea4:      cmp x8, x9
1003baea8:      b.hs    0x1003baefc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
1003baeac:      ldr x10, [x20, #0x28]
1003baeb0:      mov x11, #0x2600            ; =9728
1003baeb4:      movk    x11, #0x1, lsl #32
1003baeb8:      mov w1, #0x4                ; =4
1003baebc:      ldrb    w12, [x10, x8]
1003baec0:      cmp w12, #0x20
1003baec4:      b.hi    0x1003baefc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
1003baec8:      lsr x12, x11, x12
1003baecc:      tbz w12, #0x0, 0x1003baefc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x36c>
1003baed0:      add x8, x8, #0x1
1003baed4:      str x8, [x20, #0x38]
1003baed8:      cmp x9, x8
1003baedc:      b.ne    0x1003baebc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x32c>
1003baee0:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003baee4:      mov w1, #0x3                ; =3
1003baee8:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003baeec:      mov w1, #0x3                ; =3
1003baef0:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003baef4:      mov w1, #0x3                ; =3
1003baef8:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003baefc:      cmp x8, x9
1003baf00:      b.hs    0x1003bafa8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x418>
1003baf04:      ldr x10, [x20, #0x28]
1003baf08:      ldrb    w11, [x10, x8]
1003baf0c:      cmp w11, #0x2c
1003baf10:      b.ne    0x1003bafbc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x42c>
1003baf14:      add x8, x8, #0x1
1003baf18:      str x8, [x20, #0x38]
1003baf1c:      mov x0, x20
1003baf20:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003baf24:      ldrb    w8, [x20, #0x90]
1003baf28:      cbz w8, 0x1003bafc4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x434>
1003baf2c:      str x0, [sp, #0x20]
1003baf30:      ldp x9, x8, [x20, #0x30]
1003baf34:      cmp x8, x9
1003baf38:      b.hs    0x1003bafcc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
1003baf3c:      ldr x10, [x20, #0x28]
1003baf40:      mov x11, #0x2600            ; =9728
1003baf44:      movk    x11, #0x1, lsl #32
1003baf48:      mov w1, #0x5                ; =5
1003baf4c:      ldrb    w12, [x10, x8]
1003baf50:      cmp w12, #0x20
1003baf54:      b.hi    0x1003bafcc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
1003baf58:      lsr x12, x11, x12
1003baf5c:      tbz w12, #0x0, 0x1003bafcc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x43c>
1003baf60:      add x8, x8, #0x1
1003baf64:      str x8, [x20, #0x38]
1003baf68:      cmp x9, x8
1003baf6c:      b.ne    0x1003baf4c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x3bc>
1003baf70:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003baf74:      cmp w8, #0x1
1003baf78:      b.ne    0x1003bafb0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x420>
1003baf7c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003baf80:      add x1, x1, #0x824
1003baf84:      mov x21, x0
1003baf88:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003baf8c:      mov x0, x21
1003baf90:      strb    wzr, [x21, #0x20]
1003baf94:      ldr x8, [x21]
1003baf98:      cbz x8, 0x1003bac24 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x94>
1003baf9c:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003bafa0:      add x0, x0, #0xe58
1003bafa4:      bl  0x100c8d1ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1003bafa8:      mov w1, #0x4                ; =4
1003bafac:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bafb0:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003bafb4:      add x0, x0, #0xed8
1003bafb8:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1003bafbc:      mov w1, #0x4                ; =4
1003bafc0:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bafc4:      mov w1, #0x4                ; =4
1003bafc8:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003bafcc:      cmp x8, x9
1003bafd0:      b.hs    0x1003bb044 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4b4>
1003bafd4:      ldr x10, [x20, #0x28]
1003bafd8:      ldrb    w11, [x10, x8]
1003bafdc:      cmp w11, #0x2c
1003bafe0:      b.ne    0x1003bb04c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4bc>
1003bafe4:      add x8, x8, #0x1
1003bafe8:      str x8, [x20, #0x38]
1003bafec:      mov x0, x20
1003baff0:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003baff4:      ldrb    w8, [x20, #0x90]
1003baff8:      cbz w8, 0x1003bb054 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4c4>
1003baffc:      str x0, [sp, #0x28]
1003bb000:      ldp x9, x8, [x20, #0x30]
1003bb004:      cmp x8, x9
1003bb008:      b.hs    0x1003bb05c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
1003bb00c:      ldr x10, [x20, #0x28]
1003bb010:      mov x11, #0x2600            ; =9728
1003bb014:      movk    x11, #0x1, lsl #32
1003bb018:      mov w1, #0x6                ; =6
1003bb01c:      ldrb    w12, [x10, x8]
1003bb020:      cmp w12, #0x20
1003bb024:      b.hi    0x1003bb05c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
1003bb028:      lsr x12, x11, x12
1003bb02c:      tbz w12, #0x0, 0x1003bb05c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x4cc>
1003bb030:      add x8, x8, #0x1
1003bb034:      str x8, [x20, #0x38]
1003bb038:      cmp x9, x8
1003bb03c:      b.ne    0x1003bb01c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x48c>
1003bb040:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb044:      mov w1, #0x5                ; =5
1003bb048:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb04c:      mov w1, #0x5                ; =5
1003bb050:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bb054:      mov w1, #0x5                ; =5
1003bb058:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003bb05c:      cmp x8, x9
1003bb060:      b.hs    0x1003bb0d4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x544>
1003bb064:      ldr x10, [x20, #0x28]
1003bb068:      ldrb    w11, [x10, x8]
1003bb06c:      cmp w11, #0x2c
1003bb070:      b.ne    0x1003bb0dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x54c>
1003bb074:      add x8, x8, #0x1
1003bb078:      str x8, [x20, #0x38]
1003bb07c:      mov x0, x20
1003bb080:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003bb084:      ldrb    w8, [x20, #0x90]
1003bb088:      cbz w8, 0x1003bb0e4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x554>
1003bb08c:      str x0, [sp, #0x30]
1003bb090:      ldp x9, x8, [x20, #0x30]
1003bb094:      cmp x8, x9
1003bb098:      b.hs    0x1003bb0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
1003bb09c:      ldr x10, [x20, #0x28]
1003bb0a0:      mov x11, #0x2600            ; =9728
1003bb0a4:      movk    x11, #0x1, lsl #32
1003bb0a8:      mov w1, #0x7                ; =7
1003bb0ac:      ldrb    w12, [x10, x8]
1003bb0b0:      cmp w12, #0x20
1003bb0b4:      b.hi    0x1003bb0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
1003bb0b8:      lsr x12, x11, x12
1003bb0bc:      tbz w12, #0x0, 0x1003bb0ec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x55c>
1003bb0c0:      add x8, x8, #0x1
1003bb0c4:      str x8, [x20, #0x38]
1003bb0c8:      cmp x9, x8
1003bb0cc:      b.ne    0x1003bb0ac <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x51c>
1003bb0d0:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb0d4:      mov w1, #0x6                ; =6
1003bb0d8:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb0dc:      mov w1, #0x6                ; =6
1003bb0e0:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bb0e4:      mov w1, #0x6                ; =6
1003bb0e8:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003bb0ec:      cmp x8, x9
1003bb0f0:      b.hs    0x1003bb164 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5d4>
1003bb0f4:      ldr x10, [x20, #0x28]
1003bb0f8:      ldrb    w11, [x10, x8]
1003bb0fc:      cmp w11, #0x2c
1003bb100:      b.ne    0x1003bb16c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5dc>
1003bb104:      add x8, x8, #0x1
1003bb108:      str x8, [x20, #0x38]
1003bb10c:      mov x0, x20
1003bb110:      bl  0x1003b9e0c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_value>
1003bb114:      ldrb    w8, [x20, #0x90]
1003bb118:      cbz w8, 0x1003bb174 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5e4>
1003bb11c:      str x0, [sp, #0x38]
1003bb120:      ldp x9, x8, [x20, #0x30]
1003bb124:      cmp x8, x9
1003bb128:      b.hs    0x1003bb17c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
1003bb12c:      ldr x10, [x20, #0x28]
1003bb130:      mov x11, #0x2600            ; =9728
1003bb134:      movk    x11, #0x1, lsl #32
1003bb138:      mov w1, #0x8                ; =8
1003bb13c:      ldrb    w12, [x10, x8]
1003bb140:      cmp w12, #0x20
1003bb144:      b.hi    0x1003bb17c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
1003bb148:      lsr x12, x11, x12
1003bb14c:      tbz w12, #0x0, 0x1003bb17c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ec>
1003bb150:      add x8, x8, #0x1
1003bb154:      str x8, [x20, #0x38]
1003bb158:      cmp x9, x8
1003bb15c:      b.ne    0x1003bb13c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x5ac>
1003bb160:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb164:      mov w1, #0x7                ; =7
1003bb168:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb16c:      mov w1, #0x7                ; =7
1003bb170:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bb174:      mov w1, #0x7                ; =7
1003bb178:      b   0x1003bac9c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x10c>
1003bb17c:      cmp x8, x9
1003bb180:      b.hs    0x1003bb344 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7b4>
1003bb184:      ldr x10, [x20, #0x28]
1003bb188:      ldrb    w11, [x10, x8]
1003bb18c:      cmp w11, #0x2c
1003bb190:      b.ne    0x1003bb34c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7bc>
1003bb194:      add x8, x8, #0x1
1003bb198:      str x8, [x20, #0x38]
1003bb19c:      mov w0, #0x10               ; =16
1003bb1a0:      bl  0x10042fcac <_js_array_alloc>
1003bb1a4:      mov x21, x0
1003bb1a8:      mov x25, #0x0               ; =0
1003bb1ac:      mov x27, sp
1003bb1b0:      mov w28, #0x7ffe            ; =32766
1003bb1b4:      mov x8, #0x7ff8000000000000 ; =9221120237041090560
1003bb1b8:      fmov    d8, x8
1003bb1bc:      lsr x26, x0, #3
1003bb1c0:      b   0x1003bb1d8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
1003bb1c4:      add w8, w22, #0x1
1003bb1c8:      str w8, [x21]
1003bb1cc:      add x25, x25, #0x8
1003bb1d0:      cmp x25, #0x40
1003bb1d4:      b.eq    0x1003bb354 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
1003bb1d8:      ldr x23, [x27, x25]
1003bb1dc:      ldp w22, w8, [x21]
1003bb1e0:      cmp w22, w8
1003bb1e4:      b.hs    0x1003bb21c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x68c>
1003bb1e8:      add x8, x21, #0x8
1003bb1ec:      add x24, x8, x22, lsl #3
1003bb1f0:      str x23, [x24]
1003bb1f4:      mov x0, x21
1003bb1f8:      bl  0x1003fc3c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header20array_numeric_layout>
1003bb1fc:      tbz w0, #0x0, 0x1003bb238 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6a8>
1003bb200:      cmp x28, x23, lsr #48
1003bb204:      b.ne    0x1003bb258 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6c8>
1003bb208:      mov x0, x23
1003bb20c:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003bb210:      tbnz    w0, #0x0, 0x1003bb274 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
1003bb214:      scvtf   d0, w23
1003bb218:      b   0x1003bb270 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e0>
1003bb21c:      fmov    d0, x23
1003bb220:      mov x0, x21
1003bb224:      bl  0x1005e80b4 <_js_array_push_f64>
1003bb228:      add x25, x25, #0x8
1003bb22c:      cmp x25, #0x40
1003bb230:      b.ne    0x1003bb1d8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x648>
1003bb234:      b   0x1003bb354 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x7c4>
1003bb238:      cmp x26, #0x201
1003bb23c:      b.lo    0x1003bb274 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
1003bb240:      ldurb   w8, [x21, #-0x8]
1003bb244:      cmp w8, #0x1
1003bb248:      b.ne    0x1003bb274 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
1003bb24c:      ldurh   w8, [x21, #-0x6]
1003bb250:      tbnz    w8, #0xc, 0x1003bb200 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x670>
1003bb254:      b   0x1003bb274 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
1003bb258:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1003bb25c:      cmp x23, x8
1003bb260:      b.gt    0x1003bb274 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x6e4>
1003bb264:      fmov    d0, x23
1003bb268:      fcmp    d0, d0
1003bb26c:      fcsel   d0, d8, d0, vs
1003bb270:      fmov    x23, d0
1003bb274:      str x23, [x24]
1003bb278:      cmp x28, x23, lsr #48
1003bb27c:      b.ne    0x1003bb294 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x704>
1003bb280:      mov x0, x23
1003bb284:      bl  0x10069fafc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry12registration22is_class_id_registered>
1003bb288:      tbnz    w0, #0x0, 0x1003bb2a0 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x710>
1003bb28c:      scvtf   d0, w23
1003bb290:      b   0x1003bb2e8 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x758>
1003bb294:      mov x8, #0x7ff8ffffffffffff ; =9221401712017801215
1003bb298:      cmp x23, x8
1003bb29c:      b.le    0x1003bb2dc <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x74c>
1003bb2a0:      cmp x26, #0x201
1003bb2a4:      b.lo    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2a8:      ldurb   w8, [x21, #-0x8]
1003bb2ac:      cmp w8, #0x1
1003bb2b0:      b.ne    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2b4:      ldurh   w8, [x21, #-0x6]
1003bb2b8:      mov w9, #0xef7f             ; =61311
1003bb2bc:      and w9, w8, w9
1003bb2c0:      sturh   w9, [x21, #-0x6]
1003bb2c4:      mov w9, #0x1080             ; =4224
1003bb2c8:      tst w8, w9
1003bb2cc:      b.eq    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2d0:      mov x0, x21
1003bb2d4:      bl  0x10055a3e8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback32invalidate_representation_change>
1003bb2d8:      b   0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2dc:      fmov    d0, x23
1003bb2e0:      fcmp    d0, d0
1003bb2e4:      fcsel   d0, d8, d0, vs
1003bb2e8:      cmp x26, #0x201
1003bb2ec:      b.lo    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2f0:      ldurb   w8, [x21, #-0x8]
1003bb2f4:      cmp w8, #0x1
1003bb2f8:      b.ne    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb2fc:      ldurh   w8, [x21, #-0x6]
1003bb300:      tbz w8, #0x7, 0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb304:      ldr w8, [x21]
1003bb308:      cmp w22, w8
1003bb30c:      b.hs    0x1003bb314 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x784>
1003bb310:      str d0, [x24]
1003bb314:      mov x0, x21
1003bb318:      mov x1, x22
1003bb31c:      mov x2, x23
1003bb320:      bl  0x10058a300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6layout16layout_note_slot>
1003bb324:      mov x0, x21
1003bb328:      bl  0x1003ead08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena5stats18pointer_in_old_gen>
1003bb32c:      cbz w0, 0x1003bb1c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
1003bb330:      mov x0, x21
1003bb334:      mov x1, x24
1003bb338:      mov x2, x23
1003bb33c:      bl  0x10098a504 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc13barrier_store26runtime_write_barrier_slot>
1003bb340:      b   0x1003bb1c4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x634>
1003bb344:      mov w1, #0x8                ; =8
1003bb348:      b   0x1003bae28 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x298>
1003bb34c:      mov w1, #0x8                ; =8
1003bb350:      b   0x1003bad60 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix+0x1d0>
1003bb354:      mov x0, x20
1003bb358:      mov x1, x21
1003bb35c:      mov x2, x19
1003bb360:      ldp x29, x30, [sp, #0xa0]
1003bb364:      ldp x20, x19, [sp, #0x90]
1003bb368:      ldp x22, x21, [sp, #0x80]
1003bb36c:      ldp x24, x23, [sp, #0x70]
1003bb370:      ldp x26, x25, [sp, #0x60]
1003bb374:      ldp x28, x27, [sp, #0x50]
1003bb378:      ldp d9, d8, [sp, #0x40]
1003bb37c:      add sp, sp, #0xb0
1003bb380:      b   0x1003ba384 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
