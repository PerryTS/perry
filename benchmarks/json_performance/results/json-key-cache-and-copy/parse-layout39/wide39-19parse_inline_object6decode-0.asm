/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/wide39-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c1df0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode>:
1004c1df0:      sub sp, sp, #0x1c0
1004c1df4:      stp x28, x27, [sp, #0x160]
1004c1df8:      stp x26, x25, [sp, #0x170]
1004c1dfc:      stp x24, x23, [sp, #0x180]
1004c1e00:      stp x22, x21, [sp, #0x190]
1004c1e04:      stp x20, x19, [sp, #0x1a0]
1004c1e08:      stp x29, x30, [sp, #0x1b0]
1004c1e0c:      add x29, sp, #0x1b0
1004c1e10:      mov x19, x2
1004c1e14:      mov x8, #0x0                ; =0
1004c1e18:      add x9, sp, #0x48
1004c1e1c:      add x22, x9, #0x10
1004c1e20:      add x23, x9, #0x20
1004c1e24:      add x24, x9, #0x30
1004c1e28:      add x25, x9, #0x40
1004c1e2c:      add x26, x9, #0x50
1004c1e30:      add x27, x9, #0x60
1004c1e34:      add x28, x9, #0x70
1004c1e38:      mov x9, #0x2600             ; =9728
1004c1e3c:      movk    x9, #0x1, lsl #32
1004c1e40:      ldrb    w10, [x1, x8]
1004c1e44:      cmp w10, #0x20
1004c1e48:      b.hi    0x1004c1e64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c1e4c:      lsr x11, x9, x10
1004c1e50:      tbz w11, #0x0, 0x1004c1e64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x74>
1004c1e54:      add x8, x8, #0x1
1004c1e58:      cmp x19, x8
1004c1e5c:      b.ne    0x1004c1e40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x50>
1004c1e60:      b   0x1004c2468 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c1e64:      cmp w10, #0x7b
1004c1e68:      b.ne    0x1004c2468 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x678>
1004c1e6c:      stp x0, x1, [sp, #0x30]
1004c1e70:      add x20, x8, #0x1
1004c1e74:      str x20, [sp, #0x40]
1004c1e78:      adrp    x8, 0x100c2b000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x58a8>
1004c1e7c:      add x8, x8, #0xcf0
1004c1e80:      add x0, sp, #0x48
1004c1e84:      mov x1, x8
1004c1e88:      mov w2, #0x80               ; =128
1004c1e8c:      bl  0x100af9190 <_writev+0x100af9190>
1004c1e90:      ldr x0, [sp, #0x38]
1004c1e94:      mov x12, #0x0               ; =0
1004c1e98:      str xzr, [sp, #0xc8]
1004c1e9c:      mov x21, #0x2600            ; =9728
1004c1ea0:      movk    x21, #0x1, lsl #32
1004c1ea4:      cmp x20, x19
1004c1ea8:      b.hs    0x1004c1ed0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1eac:      ldrb    w8, [x0, x20]
1004c1eb0:      cmp w8, #0x20
1004c1eb4:      b.hi    0x1004c1ed0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1eb8:      lsr x8, x21, x8
1004c1ebc:      tbz w8, #0x0, 0x1004c1ed0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xe0>
1004c1ec0:      add x20, x20, #0x1
1004c1ec4:      str x20, [sp, #0x40]
1004c1ec8:      cmp x20, x19
1004c1ecc:      b.lo    0x1004c1eac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xbc>
1004c1ed0:      str x12, [sp, #0x28]
1004c1ed4:      add x2, sp, #0x40
1004c1ed8:      mov x1, x19
1004c1edc:      bl  0x1004c1b3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c1ee0:      cmp x0, #0x1
1004c1ee4:      b.ne    0x1004c248c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x69c>
1004c1ee8:      ldp x0, x8, [sp, #0x38]
1004c1eec:      cmp x8, x19
1004c1ef0:      ldr x11, [sp, #0x30]
1004c1ef4:      b.hs    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ef8:      ldrb    w9, [x0, x8]
1004c1efc:      cmp w9, #0x3a
1004c1f00:      b.hi    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f04:      lsr x10, x21, x9
1004c1f08:      tbz w10, #0x0, 0x1004c1f1c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x12c>
1004c1f0c:      add x8, x8, #0x1
1004c1f10:      cmp x19, x8
1004c1f14:      b.ne    0x1004c1ef8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x108>
1004c1f18:      b   0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f1c:      cmp x9, #0x3a
1004c1f20:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1f24:      str x22, [sp, #0x20]
1004c1f28:      add x8, x8, #0x1
1004c1f2c:      cmp x8, x19
1004c1f30:      b.hs    0x1004c1f80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c1f34:      ldrb    w9, [x0, x8]
1004c1f38:      cmp w9, #0x20
1004c1f3c:      b.hi    0x1004c1f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1f40:      lsr x10, x21, x9
1004c1f44:      tbz w10, #0x0, 0x1004c1f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x170>
1004c1f48:      add x8, x8, #0x1
1004c1f4c:      cmp x19, x8
1004c1f50:      b.ne    0x1004c1f34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x144>
1004c1f54:      mov x8, x19
1004c1f58:      mov x9, x19
1004c1f5c:      b   0x1004c1fc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1f60:      str x8, [sp, #0x40]
1004c1f64:      cmp w9, #0x22
1004c1f68:      b.ne    0x1004c1f80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x190>
1004c1f6c:      add x2, sp, #0x40
1004c1f70:      mov x20, x1
1004c1f74:      mov x1, x19
1004c1f78:      bl  0x1004c1b3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object13inline_string>
1004c1f7c:      b   0x1004c2124 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x334>
1004c1f80:      mov x9, x8
1004c1f84:      cmp x8, x19
1004c1f88:      b.hs    0x1004c1fc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d4>
1004c1f8c:      ldrb    w10, [x0, x9]
1004c1f90:      cmp w10, #0x2c
1004c1f94:      b.hi    0x1004c1fa8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1b8>
1004c1f98:      mov x12, #0x2600            ; =9728
1004c1f9c:      movk    x12, #0x1001, lsl #32
1004c1fa0:      lsr x12, x12, x10
1004c1fa4:      tbnz    w12, #0x0, 0x1004c1fc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1fa8:      cmp w10, #0x7d
1004c1fac:      b.eq    0x1004c1fc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1fb0:      add x9, x9, #0x1
1004c1fb4:      cmp x19, x9
1004c1fb8:      b.ne    0x1004c1f8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x19c>
1004c1fbc:      mov x9, x19
1004c1fc0:      b   0x1004c1fc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x1d8>
1004c1fc4:      mov x9, x8
1004c1fc8:      str x9, [sp, #0x40]
1004c1fcc:      cmp x9, x8
1004c1fd0:      b.lo    0x1004c25a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c1fd4:      cmp x9, x19
1004c1fd8:      b.hi    0x1004c25a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7b4>
1004c1fdc:      subs    x12, x9, x8
1004c1fe0:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1fe4:      add x20, x0, x8
1004c1fe8:      ldrb    w13, [x20]
1004c1fec:      orr w10, w13, #0x20
1004c1ff0:      cmp w10, #0x7b
1004c1ff4:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c1ff8:      mov x22, #0x0               ; =0
1004c1ffc:      sub x10, x12, #0x1
1004c2000:      sub x12, x12, #0x2
1004c2004:      cmp w13, #0x20
1004c2008:      b.hi    0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c200c:      mov w14, w13
1004c2010:      lsr x14, x21, x14
1004c2014:      tbz w14, #0x0, 0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x24c>
1004c2018:      cmp x10, x22
1004c201c:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2020:      add x13, x20, x22
1004c2024:      ldrb    w13, [x13, #0x1]
1004c2028:      add x22, x22, #0x1
1004c202c:      add x8, x8, #0x1
1004c2030:      sub x12, x12, #0x1
1004c2034:      cmp w13, #0x20
1004c2038:      b.ls    0x1004c200c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x21c>
1004c203c:      sub x10, x0, #0x1
1004c2040:      mov x14, x8
1004c2044:      ldrb    w15, [x10, x9]
1004c2048:      cmp w15, #0x20
1004c204c:      b.hi    0x1004c2070 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c2050:      lsr x16, x21, x15
1004c2054:      tbz w16, #0x0, 0x1004c2070 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x280>
1004c2058:      sub x10, x10, #0x1
1004c205c:      add x14, x14, #0x1
1004c2060:      sub x12, x12, #0x1
1004c2064:      cmp x9, x14
1004c2068:      b.ne    0x1004c2044 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x254>
1004c206c:      b   0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2070:      sub x10, x9, x14
1004c2074:      cmp x10, #0x4
1004c2078:      b.eq    0x1004c20f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x300>
1004c207c:      cmp x10, #0x5
1004c2080:      b.ne    0x1004c20f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x308>
1004c2084:      cmp w13, #0x22
1004c2088:      b.eq    0x1004c2268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c208c:      cmp w13, #0x2d
1004c2090:      b.eq    0x1004c2114 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c2094:      cmp w13, #0x66
1004c2098:      b.ne    0x1004c2108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c209c:      add x8, x20, x22
1004c20a0:      ldrb    w9, [x8, #0x1]
1004c20a4:      cmp w9, #0x61
1004c20a8:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20ac:      ldrb    w8, [x8, #0x2]
1004c20b0:      cmp w8, #0x6c
1004c20b4:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20b8:      add x8, x20, x22
1004c20bc:      ldrb    w9, [x8, #0x3]
1004c20c0:      cmp w9, #0x73
1004c20c4:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20c8:      ldrb    w8, [x8, #0x4]
1004c20cc:      cmp w8, #0x65
1004c20d0:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c20d4:      mov x8, #0x2                ; =2
1004c20d8:      movk    x8, #0x7ffc, lsl #48
1004c20dc:      orr x8, x8, #0x1
1004c20e0:      ldp x22, x12, [sp, #0x20]
1004c20e4:      cmp x12, #0x9
1004c20e8:      b.lo    0x1004c2144 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c20ec:      b   0x1004c2450 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c20f0:      cmp w13, #0x6d
1004c20f4:      b.gt    0x1004c2318 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x528>
1004c20f8:      cmp w13, #0x22
1004c20fc:      b.eq    0x1004c2268 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x478>
1004c2100:      cmp w13, #0x2d
1004c2104:      b.eq    0x1004c2114 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x324>
1004c2108:      sub w8, w13, #0x30
1004c210c:      cmp w8, #0xa
1004c2110:      b.hs    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2114:      add x0, x20, x22
1004c2118:      mov x20, x1
1004c211c:      mov x1, x10
1004c2120:      bl  0x1004bc348 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004c2124:      mov x9, x0
1004c2128:      ldp x11, x0, [sp, #0x30]
1004c212c:      tbz w9, #0x0, 0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2130:      mov x8, x1
1004c2134:      mov x1, x20
1004c2138:      ldp x22, x12, [sp, #0x20]
1004c213c:      cmp x12, #0x9
1004c2140:      b.hs    0x1004c2450 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c2144:      cbz x12, 0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2148:      ldr x9, [sp, #0x48]
1004c214c:      cmp x9, x1
1004c2150:      b.ne    0x1004c2160 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x370>
1004c2154:      add x9, sp, #0x48
1004c2158:      str x8, [x9, #0x8]
1004c215c:      b   0x1004c2180 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x390>
1004c2160:      cmp x12, #0x1
1004c2164:      b.ne    0x1004c21bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3cc>
1004c2168:      add x9, sp, #0x48
1004c216c:      add x9, x9, x12, lsl #4
1004c2170:      stp x1, x8, [x9]
1004c2174:      ldr x8, [sp, #0xc8]
1004c2178:      add x12, x8, #0x1
1004c217c:      str x12, [sp, #0xc8]
1004c2180:      ldr x20, [sp, #0x40]
1004c2184:      cmp x20, x19
1004c2188:      b.hs    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c218c:      ldrb    w8, [x0, x20]
1004c2190:      cmp w8, #0x2c
1004c2194:      b.hi    0x1004c2498 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c2198:      lsr x9, x21, x8
1004c219c:      tbz w9, #0x0, 0x1004c21b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x3c0>
1004c21a0:      add x20, x20, #0x1
1004c21a4:      cmp x19, x20
1004c21a8:      b.ne    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x39c>
1004c21ac:      b   0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c21b0:      cmp x8, #0x2c
1004c21b4:      b.eq    0x1004c1ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0xd0>
1004c21b8:      b   0x1004c2498 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6a8>
1004c21bc:      ldr x10, [sp, #0x58]
1004c21c0:      mov x9, x22
1004c21c4:      cmp x10, x1
1004c21c8:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c21cc:      cmp x12, #0x2
1004c21d0:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c21d4:      ldr x10, [sp, #0x68]
1004c21d8:      mov x9, x23
1004c21dc:      cmp x10, x1
1004c21e0:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c21e4:      cmp x12, #0x3
1004c21e8:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c21ec:      ldr x10, [sp, #0x78]
1004c21f0:      mov x9, x24
1004c21f4:      cmp x10, x1
1004c21f8:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c21fc:      cmp x12, #0x4
1004c2200:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2204:      ldr x10, [sp, #0x88]
1004c2208:      mov x9, x25
1004c220c:      cmp x10, x1
1004c2210:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c2214:      cmp x12, #0x5
1004c2218:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c221c:      ldr x10, [sp, #0x98]
1004c2220:      mov x9, x26
1004c2224:      cmp x10, x1
1004c2228:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c222c:      cmp x12, #0x6
1004c2230:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2234:      ldr x10, [sp, #0xa8]
1004c2238:      mov x9, x27
1004c223c:      cmp x10, x1
1004c2240:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c2244:      cmp x12, #0x7
1004c2248:      b.eq    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c224c:      ldr x10, [sp, #0xb8]
1004c2250:      mov x9, x28
1004c2254:      cmp x10, x1
1004c2258:      b.eq    0x1004c2158 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x368>
1004c225c:      cmp x12, #0x8
1004c2260:      b.ne    0x1004c2168 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x378>
1004c2264:      b   0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2268:      sub x13, x9, #0x1
1004c226c:      cmp x13, x14
1004c2270:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2274:      cmp x10, #0x7
1004c2278:      b.hi    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c227c:      cmp w15, #0x22
1004c2280:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2284:      sub x13, x9, #0x2
1004c2288:      add x9, x0, #0x1
1004c228c:      cmp x13, x14
1004c2290:      b.ne    0x1004c22ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c2294:      sub x11, x10, #0x2
1004c2298:      add x10, x20, x22
1004c229c:      add x8, sp, #0xd0
1004c22a0:      add x9, x20, x22
1004c22a4:      stp x9, x11, [sp, #0x8]
1004c22a8:      add x0, x10, #0x1
1004c22ac:      str x1, [sp, #0x18]
1004c22b0:      mov x1, x11
1004c22b4:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004c22b8:      ldr x11, [sp, #0x30]
1004c22bc:      ldr x8, [sp, #0xd0]
1004c22c0:      cbnz    x8, 0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c22c4:      ldr x8, [sp, #0x10]
1004c22c8:      cmp x8, #0x2
1004c22cc:      b.gt    0x1004c236c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x57c>
1004c22d0:      cbz x8, 0x1004c23d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5e4>
1004c22d4:      cmp x8, #0x1
1004c22d8:      b.ne    0x1004c2400 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x610>
1004c22dc:      ldr x8, [sp, #0x8]
1004c22e0:      ldurb   w8, [x8, #0x1]
1004c22e4:      mov x9, #0x10000000000      ; =1099511627776
1004c22e8:      b   0x1004c2434 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c22ec:      ldrb    w13, [x9, x8]
1004c22f0:      cmp w13, #0x20
1004c22f4:      b.lo    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c22f8:      cmp w13, #0x22
1004c22fc:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2300:      cmp w13, #0x5c
1004c2304:      b.eq    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2308:      add x9, x9, #0x1
1004c230c:      subs    x12, x12, #0x1
1004c2310:      b.ne    0x1004c22ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4fc>
1004c2314:      b   0x1004c2294 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x4a4>
1004c2318:      cmp w13, #0x74
1004c231c:      b.eq    0x1004c238c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x59c>
1004c2320:      cmp w13, #0x6e
1004c2324:      b.ne    0x1004c2108 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x318>
1004c2328:      add x8, x20, x22
1004c232c:      ldrb    w9, [x8, #0x1]
1004c2330:      cmp w9, #0x75
1004c2334:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2338:      ldrb    w8, [x8, #0x2]
1004c233c:      cmp w8, #0x6c
1004c2340:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2344:      add x8, x20, x22
1004c2348:      ldrb    w8, [x8, #0x3]
1004c234c:      cmp w8, #0x6c
1004c2350:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c2354:      mov x8, #0x2                ; =2
1004c2358:      movk    x8, #0x7ffc, lsl #48
1004c235c:      ldp x22, x12, [sp, #0x20]
1004c2360:      cmp x12, #0x9
1004c2364:      b.lo    0x1004c2144 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c2368:      b   0x1004c2450 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c236c:      cmp x8, #0x3
1004c2370:      b.eq    0x1004c23dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x5ec>
1004c2374:      cmp x8, #0x4
1004c2378:      b.ne    0x1004c241c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x62c>
1004c237c:      ldr x8, [sp, #0x8]
1004c2380:      ldur    w8, [x8, #0x1]
1004c2384:      mov x9, #0x40000000000      ; =4398046511104
1004c2388:      b   0x1004c2434 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c238c:      add x8, x20, x22
1004c2390:      ldrb    w9, [x8, #0x1]
1004c2394:      cmp w9, #0x72
1004c2398:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c239c:      ldrb    w8, [x8, #0x2]
1004c23a0:      cmp w8, #0x75
1004c23a4:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c23a8:      add x8, x20, x22
1004c23ac:      ldrb    w8, [x8, #0x3]
1004c23b0:      cmp w8, #0x65
1004c23b4:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c23b8:      mov x8, #0x2                ; =2
1004c23bc:      movk    x8, #0x7ffc, lsl #48
1004c23c0:      add x8, x8, #0x2
1004c23c4:      ldp x22, x12, [sp, #0x20]
1004c23c8:      cmp x12, #0x9
1004c23cc:      b.lo    0x1004c2144 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c23d0:      b   0x1004c2450 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x660>
1004c23d4:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004c23d8:      b   0x1004c243c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x64c>
1004c23dc:      add x8, x20, x22
1004c23e0:      ldurb   w8, [x8, #0x1]
1004c23e4:      add x9, x20, x22
1004c23e8:      ldrb    w10, [x9, #0x2]
1004c23ec:      ldrb    w9, [x9, #0x3]
1004c23f0:      orr x8, x8, x10, lsl #8
1004c23f4:      orr x8, x8, x9, lsl #16
1004c23f8:      mov x9, #0x30000000000      ; =3298534883328
1004c23fc:      b   0x1004c2434 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c2400:      add x8, x20, x22
1004c2404:      ldurb   w8, [x8, #0x1]
1004c2408:      add x9, x20, x22
1004c240c:      ldrb    w9, [x9, #0x2]
1004c2410:      orr x8, x8, x9, lsl #8
1004c2414:      mov x9, #0x20000000000      ; =2199023255552
1004c2418:      b   0x1004c2434 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x644>
1004c241c:      add x8, x20, x22
1004c2420:      ldur    w8, [x8, #0x1]
1004c2424:      add x9, x20, x22
1004c2428:      ldrb    w9, [x9, #0x5]
1004c242c:      orr x8, x8, x9, lsl #32
1004c2430:      mov x9, #0x50000000000      ; =5497558138880
1004c2434:      movk    x9, #0x7ff9, lsl #48
1004c2438:      orr x8, x8, x9
1004c243c:      ldp x11, x0, [sp, #0x30]
1004c2440:      ldp x1, x22, [sp, #0x18]
1004c2444:      ldr x12, [sp, #0x28]
1004c2448:      cmp x12, #0x9
1004c244c:      b.lo    0x1004c2144 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x354>
1004c2450:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004c2454:      add x3, x3, #0xfe8
1004c2458:      mov x0, #0x0                ; =0
1004c245c:      mov x1, x12
1004c2460:      mov w2, #0x8                ; =8
1004c2464:      bl  0x100ab180c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c2468:      str xzr, [x0]
1004c246c:      ldp x29, x30, [sp, #0x1b0]
1004c2470:      ldp x20, x19, [sp, #0x1a0]
1004c2474:      ldp x22, x21, [sp, #0x190]
1004c2478:      ldp x24, x23, [sp, #0x180]
1004c247c:      ldp x26, x25, [sp, #0x170]
1004c2480:      ldp x28, x27, [sp, #0x160]
1004c2484:      add sp, sp, #0x1c0
1004c2488:      ret
1004c248c:      ldr x8, [sp, #0x30]
1004c2490:      str xzr, [x8]
1004c2494:      b   0x1004c246c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c2498:      cmp w8, #0x7d
1004c249c:      b.ne    0x1004c2510 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x720>
1004c24a0:      add x8, x20, #0x1
1004c24a4:      cmp x8, x19
1004c24a8:      b.hs    0x1004c2518 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c24ac:      mov x9, #0x2600             ; =9728
1004c24b0:      movk    x9, #0x1, lsl #32
1004c24b4:      ldrb    w10, [x0, x8]
1004c24b8:      cmp w10, #0x20
1004c24bc:      b.hi    0x1004c2518 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c24c0:      lsr x10, x9, x10
1004c24c4:      tbz w10, #0x0, 0x1004c2518 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x728>
1004c24c8:      add x8, x8, #0x1
1004c24cc:      cmp x19, x8
1004c24d0:      b.ne    0x1004c24b4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x6c4>
1004c24d4:      ldr x8, [sp, #0xc8]
1004c24d8:      str x8, [sp, #0x150]
1004c24dc:      ldur    q0, [sp, #0x88]
1004c24e0:      ldur    q1, [sp, #0x98]
1004c24e4:      stp q0, q1, [sp, #0x110]
1004c24e8:      ldur    q0, [sp, #0xa8]
1004c24ec:      ldur    q1, [sp, #0xb8]
1004c24f0:      stp q0, q1, [sp, #0x130]
1004c24f4:      ldur    q0, [sp, #0x48]
1004c24f8:      ldur    q1, [sp, #0x58]
1004c24fc:      stp q0, q1, [sp, #0xd0]
1004c2500:      ldur    q0, [sp, #0x68]
1004c2504:      ldur    q1, [sp, #0x78]
1004c2508:      stp q0, q1, [sp, #0xf0]
1004c250c:      b   0x1004c2558 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x768>
1004c2510:      str xzr, [x11]
1004c2514:      b   0x1004c246c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c2518:      ldr x9, [sp, #0xc8]
1004c251c:      str x9, [sp, #0x150]
1004c2520:      ldur    q0, [sp, #0x88]
1004c2524:      ldur    q1, [sp, #0x98]
1004c2528:      stp q0, q1, [sp, #0x110]
1004c252c:      ldur    q0, [sp, #0xa8]
1004c2530:      ldur    q1, [sp, #0xb8]
1004c2534:      stp q0, q1, [sp, #0x130]
1004c2538:      ldur    q0, [sp, #0x48]
1004c253c:      ldur    q1, [sp, #0x58]
1004c2540:      stp q0, q1, [sp, #0xd0]
1004c2544:      ldur    q0, [sp, #0x68]
1004c2548:      ldur    q1, [sp, #0x78]
1004c254c:      stp q0, q1, [sp, #0xf0]
1004c2550:      cmp x8, x19
1004c2554:      b.ne    0x1004c2598 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7a8>
1004c2558:      ldp q0, q1, [sp, #0x110]
1004c255c:      stur    q0, [x11, #0x48]
1004c2560:      stur    q1, [x11, #0x58]
1004c2564:      ldp q0, q1, [sp, #0x130]
1004c2568:      stur    q0, [x11, #0x68]
1004c256c:      stur    q1, [x11, #0x78]
1004c2570:      ldp q0, q1, [sp, #0xd0]
1004c2574:      stur    q0, [x11, #0x8]
1004c2578:      stur    q1, [x11, #0x18]
1004c257c:      ldp q0, q1, [sp, #0xf0]
1004c2580:      stur    q0, [x11, #0x28]
1004c2584:      ldr x8, [sp, #0x150]
1004c2588:      str x8, [x11, #0x88]
1004c258c:      mov w8, #0x1                ; =1
1004c2590:      stur    q1, [x11, #0x38]
1004c2594:      b   0x1004c259c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x7ac>
1004c2598:      mov x8, #0x0                ; =0
1004c259c:      str x8, [x11]
1004c25a0:      b   0x1004c246c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object6decode+0x67c>
1004c25a4:      adrp    x3, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ec0>
1004c25a8:      add x3, x3, #0xfd0
1004c25ac:      mov x0, x8
1004c25b0:      mov x1, x9
1004c25b4:      mov x2, x19
1004c25b8:      bl  0x100ab180c <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
