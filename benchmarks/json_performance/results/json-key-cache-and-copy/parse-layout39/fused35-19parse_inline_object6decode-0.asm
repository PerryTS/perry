/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c1bf0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode>:
1004c1bf0:      sub sp, sp, #0x1c0
1004c1bf4:      stp x28, x27, [sp, #0x160]
1004c1bf8:      stp x26, x25, [sp, #0x170]
1004c1bfc:      stp x24, x23, [sp, #0x180]
1004c1c00:      stp x22, x21, [sp, #0x190]
1004c1c04:      stp x20, x19, [sp, #0x1a0]
1004c1c08:      stp x29, x30, [sp, #0x1b0]
1004c1c0c:      add x29, sp, #0x1b0
1004c1c10:      mov x19, x2
1004c1c14:      mov x8, #0x0                ; =0
1004c1c18:      add x9, sp, #0x48
1004c1c1c:      add x22, x9, #0x10
1004c1c20:      add x23, x9, #0x20
1004c1c24:      add x24, x9, #0x30
1004c1c28:      add x25, x9, #0x40
1004c1c2c:      add x26, x9, #0x50
1004c1c30:      add x27, x9, #0x60
1004c1c34:      add x28, x9, #0x70
1004c1c38:      mov x9, #0x2600             ; =9728
1004c1c3c:      movk    x9, #0x1, lsl #32
1004c1c40:      ldrb    w10, [x1, x8]
1004c1c44:      cmp w10, #0x20
1004c1c48:      b.hi    0x1004c1c64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c1c4c:      lsr x11, x9, x10
1004c1c50:      tbz w11, #0x0, 0x1004c1c64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c1c54:      add x8, x8, #0x1
1004c1c58:      cmp x19, x8
1004c1c5c:      b.ne    0x1004c1c40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x50>
1004c1c60:      b   0x1004c2268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c1c64:      cmp w10, #0x7b
1004c1c68:      b.ne    0x1004c2268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c1c6c:      stp x0, x1, [sp, #0x30]
1004c1c70:      add x20, x8, #0x1
1004c1c74:      str x20, [sp, #0x40]
1004c1c78:      adrp    x8, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x5aa8>
1004c1c7c:      add x8, x8, #0xaf0
1004c1c80:      add x0, sp, #0x48
1004c1c84:      mov x1, x8
1004c1c88:      mov w2, #0x80               ; =128
1004c1c8c:      bl  0x100af8f90 <_writev+0x100af8f90>
1004c1c90:      ldr x0, [sp, #0x38]
1004c1c94:      mov x12, #0x0               ; =0
1004c1c98:      str xzr, [sp, #0xc8]
1004c1c9c:      mov x21, #0x2600            ; =9728
1004c1ca0:      movk    x21, #0x1, lsl #32
1004c1ca4:      cmp x20, x19
1004c1ca8:      b.hs    0x1004c1cd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1cac:      ldrb    w8, [x0, x20]
1004c1cb0:      cmp w8, #0x20
1004c1cb4:      b.hi    0x1004c1cd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1cb8:      lsr x8, x21, x8
1004c1cbc:      tbz w8, #0x0, 0x1004c1cd0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1cc0:      add x20, x20, #0x1
1004c1cc4:      str x20, [sp, #0x40]
1004c1cc8:      cmp x20, x19
1004c1ccc:      b.lo    0x1004c1cac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xbc>
1004c1cd0:      str x12, [sp, #0x28]
1004c1cd4:      add x2, sp, #0x40
1004c1cd8:      mov x1, x19
1004c1cdc:      bl  0x1004c193c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c1ce0:      cmp x0, #0x1
1004c1ce4:      b.ne    0x1004c228c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x69c>
1004c1ce8:      ldp x0, x8, [sp, #0x38]
1004c1cec:      cmp x8, x19
1004c1cf0:      ldr x11, [sp, #0x30]
1004c1cf4:      b.hs    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1cf8:      ldrb    w9, [x0, x8]
1004c1cfc:      cmp w9, #0x3a
1004c1d00:      b.hi    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1d04:      lsr x10, x21, x9
1004c1d08:      tbz w10, #0x0, 0x1004c1d1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x12c>
1004c1d0c:      add x8, x8, #0x1
1004c1d10:      cmp x19, x8
1004c1d14:      b.ne    0x1004c1cf8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x108>
1004c1d18:      b   0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1d1c:      cmp x9, #0x3a
1004c1d20:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1d24:      str x22, [sp, #0x20]
1004c1d28:      add x8, x8, #0x1
1004c1d2c:      cmp x8, x19
1004c1d30:      b.hs    0x1004c1d80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c1d34:      ldrb    w9, [x0, x8]
1004c1d38:      cmp w9, #0x20
1004c1d3c:      b.hi    0x1004c1d60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1d40:      lsr x10, x21, x9
1004c1d44:      tbz w10, #0x0, 0x1004c1d60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1d48:      add x8, x8, #0x1
1004c1d4c:      cmp x19, x8
1004c1d50:      b.ne    0x1004c1d34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x144>
1004c1d54:      mov x8, x19
1004c1d58:      mov x9, x19
1004c1d5c:      b   0x1004c1dc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1d60:      str x8, [sp, #0x40]
1004c1d64:      cmp w9, #0x22
1004c1d68:      b.ne    0x1004c1d80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c1d6c:      add x2, sp, #0x40
1004c1d70:      mov x20, x1
1004c1d74:      mov x1, x19
1004c1d78:      bl  0x1004c193c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c1d7c:      b   0x1004c1f24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x334>
1004c1d80:      mov x9, x8
1004c1d84:      cmp x8, x19
1004c1d88:      b.hs    0x1004c1dc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d4>
1004c1d8c:      ldrb    w10, [x0, x9]
1004c1d90:      cmp w10, #0x2c
1004c1d94:      b.hi    0x1004c1da8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1b8>
1004c1d98:      mov x12, #0x2600            ; =9728
1004c1d9c:      movk    x12, #0x1001, lsl #32
1004c1da0:      lsr x12, x12, x10
1004c1da4:      tbnz    w12, #0x0, 0x1004c1dc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1da8:      cmp w10, #0x7d
1004c1dac:      b.eq    0x1004c1dc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1db0:      add x9, x9, #0x1
1004c1db4:      cmp x19, x9
1004c1db8:      b.ne    0x1004c1d8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x19c>
1004c1dbc:      mov x9, x19
1004c1dc0:      b   0x1004c1dc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1dc4:      mov x9, x8
1004c1dc8:      str x9, [sp, #0x40]
1004c1dcc:      cmp x9, x8
1004c1dd0:      b.lo    0x1004c23a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c1dd4:      cmp x9, x19
1004c1dd8:      b.hi    0x1004c23a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c1ddc:      subs    x12, x9, x8
1004c1de0:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1de4:      add x20, x0, x8
1004c1de8:      ldrb    w13, [x20]
1004c1dec:      orr w10, w13, #0x20
1004c1df0:      cmp w10, #0x7b
1004c1df4:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1df8:      mov x22, #0x0               ; =0
1004c1dfc:      sub x10, x12, #0x1
1004c1e00:      sub x12, x12, #0x2
1004c1e04:      cmp w13, #0x20
1004c1e08:      b.hi    0x1004c1e3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c1e0c:      mov w14, w13
1004c1e10:      lsr x14, x21, x14
1004c1e14:      tbz w14, #0x0, 0x1004c1e3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c1e18:      cmp x10, x22
1004c1e1c:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1e20:      add x13, x20, x22
1004c1e24:      ldrb    w13, [x13, #0x1]
1004c1e28:      add x22, x22, #0x1
1004c1e2c:      add x8, x8, #0x1
1004c1e30:      sub x12, x12, #0x1
1004c1e34:      cmp w13, #0x20
1004c1e38:      b.ls    0x1004c1e0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x21c>
1004c1e3c:      sub x10, x0, #0x1
1004c1e40:      mov x14, x8
1004c1e44:      ldrb    w15, [x10, x9]
1004c1e48:      cmp w15, #0x20
1004c1e4c:      b.hi    0x1004c1e70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c1e50:      lsr x16, x21, x15
1004c1e54:      tbz w16, #0x0, 0x1004c1e70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c1e58:      sub x10, x10, #0x1
1004c1e5c:      add x14, x14, #0x1
1004c1e60:      sub x12, x12, #0x1
1004c1e64:      cmp x9, x14
1004c1e68:      b.ne    0x1004c1e44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x254>
1004c1e6c:      b   0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1e70:      sub x10, x9, x14
1004c1e74:      cmp x10, #0x4
1004c1e78:      b.eq    0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x300>
1004c1e7c:      cmp x10, #0x5
1004c1e80:      b.ne    0x1004c1ef8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x308>
1004c1e84:      cmp w13, #0x22
1004c1e88:      b.eq    0x1004c2068 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c1e8c:      cmp w13, #0x2d
1004c1e90:      b.eq    0x1004c1f14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c1e94:      cmp w13, #0x66
1004c1e98:      b.ne    0x1004c1f08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c1e9c:      add x8, x20, x22
1004c1ea0:      ldrb    w9, [x8, #0x1]
1004c1ea4:      cmp w9, #0x61
1004c1ea8:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1eac:      ldrb    w8, [x8, #0x2]
1004c1eb0:      cmp w8, #0x6c
1004c1eb4:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1eb8:      add x8, x20, x22
1004c1ebc:      ldrb    w9, [x8, #0x3]
1004c1ec0:      cmp w9, #0x73
1004c1ec4:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ec8:      ldrb    w8, [x8, #0x4]
1004c1ecc:      cmp w8, #0x65
1004c1ed0:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ed4:      mov x8, #0x2                ; =2
1004c1ed8:      movk    x8, #0x7ffc, lsl #48
1004c1edc:      orr x8, x8, #0x1
1004c1ee0:      ldp x22, x12, [sp, #0x20]
1004c1ee4:      cmp x12, #0x9
1004c1ee8:      b.lo    0x1004c1f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c1eec:      b   0x1004c2250 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1ef0:      cmp w13, #0x6d
1004c1ef4:      b.gt    0x1004c2118 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x528>
1004c1ef8:      cmp w13, #0x22
1004c1efc:      b.eq    0x1004c2068 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c1f00:      cmp w13, #0x2d
1004c1f04:      b.eq    0x1004c1f14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c1f08:      sub w8, w13, #0x30
1004c1f0c:      cmp w8, #0xa
1004c1f10:      b.hs    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f14:      add x0, x20, x22
1004c1f18:      mov x20, x1
1004c1f1c:      mov x1, x10
1004c1f20:      bl  0x1004bc148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004c1f24:      mov x9, x0
1004c1f28:      ldp x11, x0, [sp, #0x30]
1004c1f2c:      tbz w9, #0x0, 0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f30:      mov x8, x1
1004c1f34:      mov x1, x20
1004c1f38:      ldp x22, x12, [sp, #0x20]
1004c1f3c:      cmp x12, #0x9
1004c1f40:      b.hs    0x1004c2250 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c1f44:      cbz x12, 0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1f48:      ldr x9, [sp, #0x48]
1004c1f4c:      cmp x9, x1
1004c1f50:      b.ne    0x1004c1f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x370>
1004c1f54:      add x9, sp, #0x48
1004c1f58:      str x8, [x9, #0x8]
1004c1f5c:      b   0x1004c1f80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x390>
1004c1f60:      cmp x12, #0x1
1004c1f64:      b.ne    0x1004c1fbc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3cc>
1004c1f68:      add x9, sp, #0x48
1004c1f6c:      add x9, x9, x12, lsl #4
1004c1f70:      stp x1, x8, [x9]
1004c1f74:      ldr x8, [sp, #0xc8]
1004c1f78:      add x12, x8, #0x1
1004c1f7c:      str x12, [sp, #0xc8]
1004c1f80:      ldr x20, [sp, #0x40]
1004c1f84:      cmp x20, x19
1004c1f88:      b.hs    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f8c:      ldrb    w8, [x0, x20]
1004c1f90:      cmp w8, #0x2c
1004c1f94:      b.hi    0x1004c2298 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c1f98:      lsr x9, x21, x8
1004c1f9c:      tbz w9, #0x0, 0x1004c1fb0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3c0>
1004c1fa0:      add x20, x20, #0x1
1004c1fa4:      cmp x19, x20
1004c1fa8:      b.ne    0x1004c1f8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x39c>
1004c1fac:      b   0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1fb0:      cmp x8, #0x2c
1004c1fb4:      b.eq    0x1004c1cc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xd0>
1004c1fb8:      b   0x1004c2298 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c1fbc:      ldr x10, [sp, #0x58]
1004c1fc0:      mov x9, x22
1004c1fc4:      cmp x10, x1
1004c1fc8:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1fcc:      cmp x12, #0x2
1004c1fd0:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1fd4:      ldr x10, [sp, #0x68]
1004c1fd8:      mov x9, x23
1004c1fdc:      cmp x10, x1
1004c1fe0:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1fe4:      cmp x12, #0x3
1004c1fe8:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c1fec:      ldr x10, [sp, #0x78]
1004c1ff0:      mov x9, x24
1004c1ff4:      cmp x10, x1
1004c1ff8:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c1ffc:      cmp x12, #0x4
1004c2000:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2004:      ldr x10, [sp, #0x88]
1004c2008:      mov x9, x25
1004c200c:      cmp x10, x1
1004c2010:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c2014:      cmp x12, #0x5
1004c2018:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c201c:      ldr x10, [sp, #0x98]
1004c2020:      mov x9, x26
1004c2024:      cmp x10, x1
1004c2028:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c202c:      cmp x12, #0x6
1004c2030:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2034:      ldr x10, [sp, #0xa8]
1004c2038:      mov x9, x27
1004c203c:      cmp x10, x1
1004c2040:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c2044:      cmp x12, #0x7
1004c2048:      b.eq    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c204c:      ldr x10, [sp, #0xb8]
1004c2050:      mov x9, x28
1004c2054:      cmp x10, x1
1004c2058:      b.eq    0x1004c1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c205c:      cmp x12, #0x8
1004c2060:      b.ne    0x1004c1f68 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2064:      b   0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2068:      sub x13, x9, #0x1
1004c206c:      cmp x13, x14
1004c2070:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2074:      cmp x10, #0x7
1004c2078:      b.hi    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c207c:      cmp w15, #0x22
1004c2080:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2084:      sub x13, x9, #0x2
1004c2088:      add x9, x0, #0x1
1004c208c:      cmp x13, x14
1004c2090:      b.ne    0x1004c20ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c2094:      sub x11, x10, #0x2
1004c2098:      add x10, x20, x22
1004c209c:      add x8, sp, #0xd0
1004c20a0:      add x9, x20, x22
1004c20a4:      stp x9, x11, [sp, #0x8]
1004c20a8:      add x0, x10, #0x1
1004c20ac:      str x1, [sp, #0x18]
1004c20b0:      mov x1, x11
1004c20b4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004c20b8:      ldr x11, [sp, #0x30]
1004c20bc:      ldr x8, [sp, #0xd0]
1004c20c0:      cbnz    x8, 0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20c4:      ldr x8, [sp, #0x10]
1004c20c8:      cmp x8, #0x2
1004c20cc:      b.gt    0x1004c216c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x57c>
1004c20d0:      cbz x8, 0x1004c21d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5e4>
1004c20d4:      cmp x8, #0x1
1004c20d8:      b.ne    0x1004c2200 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x610>
1004c20dc:      ldr x8, [sp, #0x8]
1004c20e0:      ldurb   w8, [x8, #0x1]
1004c20e4:      mov x9, #0x10000000000      ; =1099511627776
1004c20e8:      b   0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c20ec:      ldrb    w13, [x9, x8]
1004c20f0:      cmp w13, #0x20
1004c20f4:      b.lo    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20f8:      cmp w13, #0x22
1004c20fc:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2100:      cmp w13, #0x5c
1004c2104:      b.eq    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2108:      add x9, x9, #0x1
1004c210c:      subs    x12, x12, #0x1
1004c2110:      b.ne    0x1004c20ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c2114:      b   0x1004c2094 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4a4>
1004c2118:      cmp w13, #0x74
1004c211c:      b.eq    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x59c>
1004c2120:      cmp w13, #0x6e
1004c2124:      b.ne    0x1004c1f08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c2128:      add x8, x20, x22
1004c212c:      ldrb    w9, [x8, #0x1]
1004c2130:      cmp w9, #0x75
1004c2134:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2138:      ldrb    w8, [x8, #0x2]
1004c213c:      cmp w8, #0x6c
1004c2140:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2144:      add x8, x20, x22
1004c2148:      ldrb    w8, [x8, #0x3]
1004c214c:      cmp w8, #0x6c
1004c2150:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2154:      mov x8, #0x2                ; =2
1004c2158:      movk    x8, #0x7ffc, lsl #48
1004c215c:      ldp x22, x12, [sp, #0x20]
1004c2160:      cmp x12, #0x9
1004c2164:      b.lo    0x1004c1f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c2168:      b   0x1004c2250 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c216c:      cmp x8, #0x3
1004c2170:      b.eq    0x1004c21dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5ec>
1004c2174:      cmp x8, #0x4
1004c2178:      b.ne    0x1004c221c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x62c>
1004c217c:      ldr x8, [sp, #0x8]
1004c2180:      ldur    w8, [x8, #0x1]
1004c2184:      mov x9, #0x40000000000      ; =4398046511104
1004c2188:      b   0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c218c:      add x8, x20, x22
1004c2190:      ldrb    w9, [x8, #0x1]
1004c2194:      cmp w9, #0x72
1004c2198:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c219c:      ldrb    w8, [x8, #0x2]
1004c21a0:      cmp w8, #0x75
1004c21a4:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c21a8:      add x8, x20, x22
1004c21ac:      ldrb    w8, [x8, #0x3]
1004c21b0:      cmp w8, #0x65
1004c21b4:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c21b8:      mov x8, #0x2                ; =2
1004c21bc:      movk    x8, #0x7ffc, lsl #48
1004c21c0:      add x8, x8, #0x2
1004c21c4:      ldp x22, x12, [sp, #0x20]
1004c21c8:      cmp x12, #0x9
1004c21cc:      b.lo    0x1004c1f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c21d0:      b   0x1004c2250 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c21d4:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004c21d8:      b   0x1004c223c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x64c>
1004c21dc:      add x8, x20, x22
1004c21e0:      ldurb   w8, [x8, #0x1]
1004c21e4:      add x9, x20, x22
1004c21e8:      ldrb    w10, [x9, #0x2]
1004c21ec:      ldrb    w9, [x9, #0x3]
1004c21f0:      orr x8, x8, x10, lsl #8
1004c21f4:      orr x8, x8, x9, lsl #16
1004c21f8:      mov x9, #0x30000000000      ; =3298534883328
1004c21fc:      b   0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c2200:      add x8, x20, x22
1004c2204:      ldurb   w8, [x8, #0x1]
1004c2208:      add x9, x20, x22
1004c220c:      ldrb    w9, [x9, #0x2]
1004c2210:      orr x8, x8, x9, lsl #8
1004c2214:      mov x9, #0x20000000000      ; =2199023255552
1004c2218:      b   0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c221c:      add x8, x20, x22
1004c2220:      ldur    w8, [x8, #0x1]
1004c2224:      add x9, x20, x22
1004c2228:      ldrb    w9, [x9, #0x5]
1004c222c:      orr x8, x8, x9, lsl #32
1004c2230:      mov x9, #0x50000000000      ; =5497558138880
1004c2234:      movk    x9, #0x7ff9, lsl #48
1004c2238:      orr x8, x8, x9
1004c223c:      ldp x11, x0, [sp, #0x30]
1004c2240:      ldp x1, x22, [sp, #0x18]
1004c2244:      ldr x12, [sp, #0x28]
1004c2248:      cmp x12, #0x9
1004c224c:      b.lo    0x1004c1f44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c2250:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c2254:      add x3, x3, #0xfb8
1004c2258:      mov x0, #0x0                ; =0
1004c225c:      mov x1, x12
1004c2260:      mov w2, #0x8                ; =8
1004c2264:      bl  0x100ab160c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c2268:      str xzr, [x0]
1004c226c:      ldp x29, x30, [sp, #0x1b0]
1004c2270:      ldp x20, x19, [sp, #0x1a0]
1004c2274:      ldp x22, x21, [sp, #0x190]
1004c2278:      ldp x24, x23, [sp, #0x180]
1004c227c:      ldp x26, x25, [sp, #0x170]
1004c2280:      ldp x28, x27, [sp, #0x160]
1004c2284:      add sp, sp, #0x1c0
1004c2288:      ret
1004c228c:      ldr x8, [sp, #0x30]
1004c2290:      str xzr, [x8]
1004c2294:      b   0x1004c226c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c2298:      cmp w8, #0x7d
1004c229c:      b.ne    0x1004c2310 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c22a0:      add x8, x20, #0x1
1004c22a4:      cmp x8, x19
1004c22a8:      b.hs    0x1004c2318 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c22ac:      mov x9, #0x2600             ; =9728
1004c22b0:      movk    x9, #0x1, lsl #32
1004c22b4:      ldrb    w10, [x0, x8]
1004c22b8:      cmp w10, #0x20
1004c22bc:      b.hi    0x1004c2318 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c22c0:      lsr x10, x9, x10
1004c22c4:      tbz w10, #0x0, 0x1004c2318 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c22c8:      add x8, x8, #0x1
1004c22cc:      cmp x19, x8
1004c22d0:      b.ne    0x1004c22b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6c4>
1004c22d4:      ldr x8, [sp, #0xc8]
1004c22d8:      str x8, [sp, #0x150]
1004c22dc:      ldur    q0, [sp, #0x88]
1004c22e0:      ldur    q1, [sp, #0x98]
1004c22e4:      stp q0, q1, [sp, #0x110]
1004c22e8:      ldur    q0, [sp, #0xa8]
1004c22ec:      ldur    q1, [sp, #0xb8]
1004c22f0:      stp q0, q1, [sp, #0x130]
1004c22f4:      ldur    q0, [sp, #0x48]
1004c22f8:      ldur    q1, [sp, #0x58]
1004c22fc:      stp q0, q1, [sp, #0xd0]
1004c2300:      ldur    q0, [sp, #0x68]
1004c2304:      ldur    q1, [sp, #0x78]
1004c2308:      stp q0, q1, [sp, #0xf0]
1004c230c:      b   0x1004c2358 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x768>
1004c2310:      str xzr, [x11]
1004c2314:      b   0x1004c226c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c2318:      ldr x9, [sp, #0xc8]
1004c231c:      str x9, [sp, #0x150]
1004c2320:      ldur    q0, [sp, #0x88]
1004c2324:      ldur    q1, [sp, #0x98]
1004c2328:      stp q0, q1, [sp, #0x110]
1004c232c:      ldur    q0, [sp, #0xa8]
1004c2330:      ldur    q1, [sp, #0xb8]
1004c2334:      stp q0, q1, [sp, #0x130]
1004c2338:      ldur    q0, [sp, #0x48]
1004c233c:      ldur    q1, [sp, #0x58]
1004c2340:      stp q0, q1, [sp, #0xd0]
1004c2344:      ldur    q0, [sp, #0x68]
1004c2348:      ldur    q1, [sp, #0x78]
1004c234c:      stp q0, q1, [sp, #0xf0]
1004c2350:      cmp x8, x19
1004c2354:      b.ne    0x1004c2398 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7a8>
1004c2358:      ldp q0, q1, [sp, #0x110]
1004c235c:      stur    q0, [x11, #0x48]
1004c2360:      stur    q1, [x11, #0x58]
1004c2364:      ldp q0, q1, [sp, #0x130]
1004c2368:      stur    q0, [x11, #0x68]
1004c236c:      stur    q1, [x11, #0x78]
1004c2370:      ldp q0, q1, [sp, #0xd0]
1004c2374:      stur    q0, [x11, #0x8]
1004c2378:      stur    q1, [x11, #0x18]
1004c237c:      ldp q0, q1, [sp, #0xf0]
1004c2380:      stur    q0, [x11, #0x28]
1004c2384:      ldr x8, [sp, #0x150]
1004c2388:      str x8, [x11, #0x88]
1004c238c:      mov w8, #0x1                ; =1
1004c2390:      stur    q1, [x11, #0x38]
1004c2394:      b   0x1004c239c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7ac>
1004c2398:      mov x8, #0x0                ; =0
1004c239c:      str x8, [x11]
1004c23a0:      b   0x1004c226c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c23a4:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c23a8:      add x3, x3, #0xfa0
1004c23ac:      mov x0, x8
1004c23b0:      mov x1, x9
1004c23b4:      mov x2, x19
1004c23b8:      bl  0x100ab160c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
