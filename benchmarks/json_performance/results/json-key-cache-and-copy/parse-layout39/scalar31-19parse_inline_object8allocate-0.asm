/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c1cec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate>:
1004c1cec:      sub sp, sp, #0xf0
1004c1cf0:      stp x28, x27, [sp, #0x90]
1004c1cf4:      stp x26, x25, [sp, #0xa0]
1004c1cf8:      stp x24, x23, [sp, #0xb0]
1004c1cfc:      stp x22, x21, [sp, #0xc0]
1004c1d00:      stp x20, x19, [sp, #0xd0]
1004c1d04:      stp x29, x30, [sp, #0xe0]
1004c1d08:      add x29, sp, #0xe0
1004c1d0c:      mov x19, x0
1004c1d10:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c1d14:      add x0, x0, #0xba8
1004c1d18:      ldr x8, [x0]
1004c1d1c:      blr x8
1004c1d20:      mov x21, x0
1004c1d24:      ldrb    w8, [x0, #0x20]
1004c1d28:      cbnz    w8, 0x1004c2744 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa58>
1004c1d2c:      ldr x28, [x21]
1004c1d30:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c1d34:      cmp x28, x8
1004c1d38:      b.hs    0x1004c2770 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa84>
1004c1d3c:      add x8, x28, #0x1
1004c1d40:      str x8, [x21]
1004c1d44:      ldr x9, [x21, #0x18]
1004c1d48:      cbz x9, 0x1004c2718 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c1d4c:      ldr x10, [x21, #0x10]
1004c1d50:      ldr x20, [x19, #0x80]
1004c1d54:      lsl x8, x9, #5
1004c1d58:      cmp x20, #0x9
1004c1d5c:      b.hs    0x1004c2700 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa14>
1004c1d60:      ldr x27, [x19]
1004c1d64:      ubfx    x22, x27, #40, #8
1004c1d68:      ldr x11, [x19, #0x10]
1004c1d6c:      str x11, [sp, #0x68]
1004c1d70:      ubfx    x23, x11, #40, #8
1004c1d74:      ldr x11, [x19, #0x20]
1004c1d78:      str x11, [sp, #0x58]
1004c1d7c:      ubfx    x11, x11, #40, #8
1004c1d80:      str x11, [sp, #0x60]
1004c1d84:      ldr x11, [x19, #0x30]
1004c1d88:      str x11, [sp, #0x48]
1004c1d8c:      ubfx    x11, x11, #40, #8
1004c1d90:      str x11, [sp, #0x50]
1004c1d94:      ldr x11, [x19, #0x40]
1004c1d98:      str x11, [sp, #0x38]
1004c1d9c:      ubfx    x11, x11, #40, #8
1004c1da0:      str x11, [sp, #0x40]
1004c1da4:      ldr x11, [x19, #0x50]
1004c1da8:      str x11, [sp, #0x28]
1004c1dac:      ubfx    x11, x11, #40, #8
1004c1db0:      str x11, [sp, #0x30]
1004c1db4:      ldr x11, [x19, #0x60]
1004c1db8:      str x11, [sp, #0x18]
1004c1dbc:      ubfx    x11, x11, #40, #8
1004c1dc0:      str x11, [sp, #0x20]
1004c1dc4:      ldr x11, [x19, #0x70]
1004c1dc8:      str x11, [sp, #0x8]
1004c1dcc:      ubfx    x11, x11, #40, #8
1004c1dd0:      str x11, [sp, #0x10]
1004c1dd4:      add x8, x8, x10
1004c1dd8:      sub x24, x8, #0x8
1004c1ddc:      neg x25, x9, lsl #5
1004c1de0:      b   0x1004c1df0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x104>
1004c1de4:      sub x24, x24, #0x20
1004c1de8:      adds    x25, x25, #0x20
1004c1dec:      b.eq    0x1004c2718 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa2c>
1004c1df0:      ldur    x8, [x24, #-0x8]
1004c1df4:      cmp x8, x20
1004c1df8:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1dfc:      sturb   wzr, [x29, #-0x64]
1004c1e00:      stur    wzr, [x29, #-0x68]
1004c1e04:      cbz x20, 0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c1e08:      ldur    x26, [x24, #-0x10]
1004c1e0c:      ldr x8, [x26]
1004c1e10:      cbz x22, 0x1004c1e60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c1e14:      sturb   w27, [x29, #-0x68]
1004c1e18:      cmp x22, #0x1
1004c1e1c:      b.eq    0x1004c1e60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c1e20:      lsr x9, x27, #8
1004c1e24:      sturb   w9, [x29, #-0x67]
1004c1e28:      cmp x22, #0x2
1004c1e2c:      b.eq    0x1004c1e60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c1e30:      lsr x9, x27, #16
1004c1e34:      sturb   w9, [x29, #-0x66]
1004c1e38:      cmp x22, #0x3
1004c1e3c:      b.eq    0x1004c1e60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c1e40:      lsr x9, x27, #24
1004c1e44:      sturb   w9, [x29, #-0x65]
1004c1e48:      cmp x22, #0x4
1004c1e4c:      b.eq    0x1004c1e60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x174>
1004c1e50:      lsr x9, x27, #32
1004c1e54:      sturb   w9, [x29, #-0x64]
1004c1e58:      cmp x22, #0x5
1004c1e5c:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c1e60:      ldr w9, [x8, #0x4]
1004c1e64:      cmp x22, x9
1004c1e68:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1e6c:      add x0, x8, #0x14
1004c1e70:      sub x1, x29, #0x68
1004c1e74:      mov x2, x22
1004c1e78:      bl  0x100af8820 <_writev+0x100af8820>
1004c1e7c:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1e80:      cmp x20, #0x1
1004c1e84:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c1e88:      ldr x8, [x26, #0x8]
1004c1e8c:      cbz x23, 0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c1e90:      ldr x9, [sp, #0x68]
1004c1e94:      sturb   w9, [x29, #-0x68]
1004c1e98:      cmp x23, #0x1
1004c1e9c:      b.eq    0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c1ea0:      ldr x9, [sp, #0x68]
1004c1ea4:      lsr x9, x9, #8
1004c1ea8:      sturb   w9, [x29, #-0x67]
1004c1eac:      cmp x23, #0x2
1004c1eb0:      b.eq    0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c1eb4:      ldr x9, [sp, #0x68]
1004c1eb8:      lsr x9, x9, #16
1004c1ebc:      sturb   w9, [x29, #-0x66]
1004c1ec0:      cmp x23, #0x3
1004c1ec4:      b.eq    0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c1ec8:      ldr x9, [sp, #0x68]
1004c1ecc:      lsr x9, x9, #24
1004c1ed0:      sturb   w9, [x29, #-0x65]
1004c1ed4:      cmp x23, #0x4
1004c1ed8:      b.eq    0x1004c1ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x204>
1004c1edc:      ldr x9, [sp, #0x68]
1004c1ee0:      lsr x9, x9, #32
1004c1ee4:      sturb   w9, [x29, #-0x64]
1004c1ee8:      cmp x23, #0x5
1004c1eec:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c1ef0:      ldr w9, [x8, #0x4]
1004c1ef4:      cmp x23, x9
1004c1ef8:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1efc:      add x0, x8, #0x14
1004c1f00:      sub x1, x29, #0x68
1004c1f04:      mov x2, x23
1004c1f08:      bl  0x100af8820 <_writev+0x100af8820>
1004c1f0c:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1f10:      cmp x20, #0x2
1004c1f14:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c1f18:      ldr x8, [x26, #0x10]
1004c1f1c:      ldr x9, [sp, #0x60]
1004c1f20:      cbz x9, 0x1004c1f94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c1f24:      ldp x10, x9, [sp, #0x58]
1004c1f28:      sturb   w10, [x29, #-0x68]
1004c1f2c:      cmp x9, #0x1
1004c1f30:      b.eq    0x1004c1f94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c1f34:      ldr x9, [sp, #0x58]
1004c1f38:      lsr x9, x9, #8
1004c1f3c:      sturb   w9, [x29, #-0x67]
1004c1f40:      ldr x9, [sp, #0x60]
1004c1f44:      cmp x9, #0x2
1004c1f48:      b.eq    0x1004c1f94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c1f4c:      ldr x9, [sp, #0x58]
1004c1f50:      lsr x9, x9, #16
1004c1f54:      sturb   w9, [x29, #-0x66]
1004c1f58:      ldr x9, [sp, #0x60]
1004c1f5c:      cmp x9, #0x3
1004c1f60:      b.eq    0x1004c1f94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c1f64:      ldr x9, [sp, #0x58]
1004c1f68:      lsr x9, x9, #24
1004c1f6c:      sturb   w9, [x29, #-0x65]
1004c1f70:      ldr x9, [sp, #0x60]
1004c1f74:      cmp x9, #0x4
1004c1f78:      b.eq    0x1004c1f94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x2a8>
1004c1f7c:      ldr x9, [sp, #0x58]
1004c1f80:      lsr x9, x9, #32
1004c1f84:      sturb   w9, [x29, #-0x64]
1004c1f88:      ldr x9, [sp, #0x60]
1004c1f8c:      cmp x9, #0x5
1004c1f90:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c1f94:      ldr w9, [x8, #0x4]
1004c1f98:      ldr x10, [sp, #0x60]
1004c1f9c:      cmp x10, x9
1004c1fa0:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1fa4:      add x0, x8, #0x14
1004c1fa8:      sub x1, x29, #0x68
1004c1fac:      ldr x2, [sp, #0x60]
1004c1fb0:      bl  0x100af8820 <_writev+0x100af8820>
1004c1fb4:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c1fb8:      cmp x20, #0x3
1004c1fbc:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c1fc0:      ldr x8, [x26, #0x18]
1004c1fc4:      ldr x9, [sp, #0x50]
1004c1fc8:      cbz x9, 0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c1fcc:      ldp x10, x9, [sp, #0x48]
1004c1fd0:      sturb   w10, [x29, #-0x68]
1004c1fd4:      cmp x9, #0x1
1004c1fd8:      b.eq    0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c1fdc:      ldr x9, [sp, #0x48]
1004c1fe0:      lsr x9, x9, #8
1004c1fe4:      sturb   w9, [x29, #-0x67]
1004c1fe8:      ldr x9, [sp, #0x50]
1004c1fec:      cmp x9, #0x2
1004c1ff0:      b.eq    0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c1ff4:      ldr x9, [sp, #0x48]
1004c1ff8:      lsr x9, x9, #16
1004c1ffc:      sturb   w9, [x29, #-0x66]
1004c2000:      ldr x9, [sp, #0x50]
1004c2004:      cmp x9, #0x3
1004c2008:      b.eq    0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c200c:      ldr x9, [sp, #0x48]
1004c2010:      lsr x9, x9, #24
1004c2014:      sturb   w9, [x29, #-0x65]
1004c2018:      ldr x9, [sp, #0x50]
1004c201c:      cmp x9, #0x4
1004c2020:      b.eq    0x1004c203c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x350>
1004c2024:      ldr x9, [sp, #0x48]
1004c2028:      lsr x9, x9, #32
1004c202c:      sturb   w9, [x29, #-0x64]
1004c2030:      ldr x9, [sp, #0x50]
1004c2034:      cmp x9, #0x5
1004c2038:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c203c:      ldr w9, [x8, #0x4]
1004c2040:      ldr x10, [sp, #0x50]
1004c2044:      cmp x10, x9
1004c2048:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c204c:      add x0, x8, #0x14
1004c2050:      sub x1, x29, #0x68
1004c2054:      ldr x2, [sp, #0x50]
1004c2058:      bl  0x100af8820 <_writev+0x100af8820>
1004c205c:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2060:      cmp x20, #0x4
1004c2064:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2068:      ldr x8, [x26, #0x20]
1004c206c:      ldr x9, [sp, #0x40]
1004c2070:      cbz x9, 0x1004c20e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2074:      ldp x10, x9, [sp, #0x38]
1004c2078:      sturb   w10, [x29, #-0x68]
1004c207c:      cmp x9, #0x1
1004c2080:      b.eq    0x1004c20e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c2084:      ldr x9, [sp, #0x38]
1004c2088:      lsr x9, x9, #8
1004c208c:      sturb   w9, [x29, #-0x67]
1004c2090:      ldr x9, [sp, #0x40]
1004c2094:      cmp x9, #0x2
1004c2098:      b.eq    0x1004c20e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c209c:      ldr x9, [sp, #0x38]
1004c20a0:      lsr x9, x9, #16
1004c20a4:      sturb   w9, [x29, #-0x66]
1004c20a8:      ldr x9, [sp, #0x40]
1004c20ac:      cmp x9, #0x3
1004c20b0:      b.eq    0x1004c20e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c20b4:      ldr x9, [sp, #0x38]
1004c20b8:      lsr x9, x9, #24
1004c20bc:      sturb   w9, [x29, #-0x65]
1004c20c0:      ldr x9, [sp, #0x40]
1004c20c4:      cmp x9, #0x4
1004c20c8:      b.eq    0x1004c20e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x3f8>
1004c20cc:      ldr x9, [sp, #0x38]
1004c20d0:      lsr x9, x9, #32
1004c20d4:      sturb   w9, [x29, #-0x64]
1004c20d8:      ldr x9, [sp, #0x40]
1004c20dc:      cmp x9, #0x5
1004c20e0:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c20e4:      ldr w9, [x8, #0x4]
1004c20e8:      ldr x10, [sp, #0x40]
1004c20ec:      cmp x10, x9
1004c20f0:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c20f4:      add x0, x8, #0x14
1004c20f8:      sub x1, x29, #0x68
1004c20fc:      ldr x2, [sp, #0x40]
1004c2100:      bl  0x100af8820 <_writev+0x100af8820>
1004c2104:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2108:      cmp x20, #0x5
1004c210c:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2110:      ldr x8, [x26, #0x28]
1004c2114:      ldr x9, [sp, #0x30]
1004c2118:      cbz x9, 0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c211c:      ldp x10, x9, [sp, #0x28]
1004c2120:      sturb   w10, [x29, #-0x68]
1004c2124:      cmp x9, #0x1
1004c2128:      b.eq    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c212c:      ldr x9, [sp, #0x28]
1004c2130:      lsr x9, x9, #8
1004c2134:      sturb   w9, [x29, #-0x67]
1004c2138:      ldr x9, [sp, #0x30]
1004c213c:      cmp x9, #0x2
1004c2140:      b.eq    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2144:      ldr x9, [sp, #0x28]
1004c2148:      lsr x9, x9, #16
1004c214c:      sturb   w9, [x29, #-0x66]
1004c2150:      ldr x9, [sp, #0x30]
1004c2154:      cmp x9, #0x3
1004c2158:      b.eq    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c215c:      ldr x9, [sp, #0x28]
1004c2160:      lsr x9, x9, #24
1004c2164:      sturb   w9, [x29, #-0x65]
1004c2168:      ldr x9, [sp, #0x30]
1004c216c:      cmp x9, #0x4
1004c2170:      b.eq    0x1004c218c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x4a0>
1004c2174:      ldr x9, [sp, #0x28]
1004c2178:      lsr x9, x9, #32
1004c217c:      sturb   w9, [x29, #-0x64]
1004c2180:      ldr x9, [sp, #0x30]
1004c2184:      cmp x9, #0x5
1004c2188:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c218c:      ldr w9, [x8, #0x4]
1004c2190:      ldr x10, [sp, #0x30]
1004c2194:      cmp x10, x9
1004c2198:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c219c:      add x0, x8, #0x14
1004c21a0:      sub x1, x29, #0x68
1004c21a4:      ldr x2, [sp, #0x30]
1004c21a8:      bl  0x100af8820 <_writev+0x100af8820>
1004c21ac:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c21b0:      cmp x20, #0x6
1004c21b4:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c21b8:      ldr x8, [x26, #0x30]
1004c21bc:      ldr x9, [sp, #0x20]
1004c21c0:      cbz x9, 0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c21c4:      ldp x10, x9, [sp, #0x18]
1004c21c8:      sturb   w10, [x29, #-0x68]
1004c21cc:      cmp x9, #0x1
1004c21d0:      b.eq    0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c21d4:      ldr x9, [sp, #0x18]
1004c21d8:      lsr x9, x9, #8
1004c21dc:      sturb   w9, [x29, #-0x67]
1004c21e0:      ldr x9, [sp, #0x20]
1004c21e4:      cmp x9, #0x2
1004c21e8:      b.eq    0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c21ec:      ldr x9, [sp, #0x18]
1004c21f0:      lsr x9, x9, #16
1004c21f4:      sturb   w9, [x29, #-0x66]
1004c21f8:      ldr x9, [sp, #0x20]
1004c21fc:      cmp x9, #0x3
1004c2200:      b.eq    0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c2204:      ldr x9, [sp, #0x18]
1004c2208:      lsr x9, x9, #24
1004c220c:      sturb   w9, [x29, #-0x65]
1004c2210:      ldr x9, [sp, #0x20]
1004c2214:      cmp x9, #0x4
1004c2218:      b.eq    0x1004c2234 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x548>
1004c221c:      ldr x9, [sp, #0x18]
1004c2220:      lsr x9, x9, #32
1004c2224:      sturb   w9, [x29, #-0x64]
1004c2228:      ldr x9, [sp, #0x20]
1004c222c:      cmp x9, #0x5
1004c2230:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c2234:      ldr w9, [x8, #0x4]
1004c2238:      ldr x10, [sp, #0x20]
1004c223c:      cmp x10, x9
1004c2240:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2244:      add x0, x8, #0x14
1004c2248:      sub x1, x29, #0x68
1004c224c:      ldr x2, [sp, #0x20]
1004c2250:      bl  0x100af8820 <_writev+0x100af8820>
1004c2254:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2258:      cmp x20, #0x7
1004c225c:      b.eq    0x1004c2300 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x614>
1004c2260:      ldr x8, [x26, #0x38]
1004c2264:      ldr x9, [sp, #0x10]
1004c2268:      cbz x9, 0x1004c22dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c226c:      ldp x10, x9, [sp, #0x8]
1004c2270:      sturb   w10, [x29, #-0x68]
1004c2274:      cmp x9, #0x1
1004c2278:      b.eq    0x1004c22dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c227c:      ldr x9, [sp, #0x8]
1004c2280:      lsr x9, x9, #8
1004c2284:      sturb   w9, [x29, #-0x67]
1004c2288:      ldr x9, [sp, #0x10]
1004c228c:      cmp x9, #0x2
1004c2290:      b.eq    0x1004c22dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c2294:      ldr x9, [sp, #0x8]
1004c2298:      lsr x9, x9, #16
1004c229c:      sturb   w9, [x29, #-0x66]
1004c22a0:      ldr x9, [sp, #0x10]
1004c22a4:      cmp x9, #0x3
1004c22a8:      b.eq    0x1004c22dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c22ac:      ldr x9, [sp, #0x8]
1004c22b0:      lsr x9, x9, #24
1004c22b4:      sturb   w9, [x29, #-0x65]
1004c22b8:      ldr x9, [sp, #0x10]
1004c22bc:      cmp x9, #0x4
1004c22c0:      b.eq    0x1004c22dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x5f0>
1004c22c4:      ldr x9, [sp, #0x8]
1004c22c8:      lsr x9, x9, #32
1004c22cc:      sturb   w9, [x29, #-0x64]
1004c22d0:      ldr x9, [sp, #0x10]
1004c22d4:      cmp x9, #0x5
1004c22d8:      b.ne    0x1004c2824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb38>
1004c22dc:      ldr w9, [x8, #0x4]
1004c22e0:      ldr x10, [sp, #0x10]
1004c22e4:      cmp x10, x9
1004c22e8:      b.ne    0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c22ec:      add x0, x8, #0x14
1004c22f0:      sub x1, x29, #0x68
1004c22f4:      ldr x2, [sp, #0x10]
1004c22f8:      bl  0x100af8820 <_writev+0x100af8820>
1004c22fc:      cbnz    w0, 0x1004c1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xf8>
1004c2300:      ldr x22, [x24]
1004c2304:      str x28, [x21]
1004c2308:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c230c:      str x0, [sp, #0x70]
1004c2310:      mov x23, #0x7ffd000000000000 ; =9222527611924643840
1004c2314:      stp x22, x23, [x29, #-0x60]
1004c2318:      mov w8, #0x1                ; =1
1004c231c:      stur    x8, [x29, #-0x68]
1004c2320:      sub x0, x29, #0x68
1004c2324:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c2328:      mov x22, x0
1004c232c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2330:      add x0, x0, #0x9d0
1004c2334:      ldr x8, [x0]
1004c2338:      blr x8
1004c233c:      mov x21, x0
1004c2340:      ldrb    w8, [x0]
1004c2344:      strb    wzr, [x0]
1004c2348:      cbz w8, 0x1004c2384 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c234c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c2350:      add x0, x0, #0x9e8
1004c2354:      ldr x8, [x0]
1004c2358:      blr x8
1004c235c:      ldrb    w8, [x0]
1004c2360:      tst w8, #0x3
1004c2364:      b.ne    0x1004c237c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x690>
1004c2368:      adrp    x8, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004c236c:      add x8, x8, #0x8fc
1004c2370:      ldapr   w8, [x8]
1004c2374:      cmp w8, #0x0
1004c2378:      b.le    0x1004c26ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9c0>
1004c237c:      mov w8, #0x1                ; =1
1004c2380:      strb    w8, [x21]
1004c2384:      mov w0, #0x0                ; =0
1004c2388:      mov w1, #0x0                ; =0
1004c238c:      mov x2, x20
1004c2390:      bl  0x10081ce80 <_js_object_alloc_with_parent>
1004c2394:      stp x0, x23, [x29, #-0x60]
1004c2398:      mov w8, #0x1                ; =1
1004c239c:      stur    x8, [x29, #-0x68]
1004c23a0:      sub x0, x29, #0x68
1004c23a4:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c23a8:      mov x23, x0
1004c23ac:      adrp    x25, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c23b0:      ldr x8, [x25, #0x48]
1004c23b4:      cmn x8, #0x1
1004c23b8:      b.eq    0x1004c241c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c23bc:      mrs x9, TPIDRRO_EL0
1004c23c0:      and x9, x9, #0xfffffffffffffff8
1004c23c4:      ldr x8, [x9, x8, lsl #3]
1004c23c8:      cbz x8, 0x1004c241c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c23cc:      ldr x8, [x8, #0x19e8]
1004c23d0:      cbz x8, 0x1004c241c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x730>
1004c23d4:      ldr x9, [x8]
1004c23d8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c23dc:      cmp x9, x10
1004c23e0:      b.hs    0x1004c27e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c23e4:      add x10, x9, #0x1
1004c23e8:      str x10, [x8]
1004c23ec:      ldr x10, [x8, #0x18]
1004c23f0:      cmp x22, x10
1004c23f4:      b.hs    0x1004c27f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c23f8:      ldr x10, [x8, #0x10]
1004c23fc:      mov w11, #0x18              ; =24
1004c2400:      madd    x10, x22, x11, x10
1004c2404:      ldr x11, [x10]
1004c2408:      cmp x11, #0x1
1004c240c:      b.ne    0x1004c27f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2410:      ldr x0, [x10, #0x8]
1004c2414:      str x9, [x8]
1004c2418:      b   0x1004c2424 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x738>
1004c241c:      mov x0, x22
1004c2420:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c2424:      mov x1, x20
1004c2428:      mov x2, x20
1004c242c:      mov x3, #0x0                ; =0
1004c2430:      mov w4, #0x0                ; =0
1004c2434:      mov w5, #0x0                ; =0
1004c2438:      bl  0x10059b2c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34shape_descriptor_ensure_with_holes>
1004c243c:      mov x24, x0
1004c2440:      tbnz    w24, #0x0, 0x1004c277c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa90>
1004c2444:      ldr x8, [x25, #0x48]
1004c2448:      cmn x8, #0x1
1004c244c:      b.eq    0x1004c24bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2450:      mrs x9, TPIDRRO_EL0
1004c2454:      and x9, x9, #0xfffffffffffffff8
1004c2458:      ldr x8, [x9, x8, lsl #3]
1004c245c:      cbz x8, 0x1004c24bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2460:      ldr x8, [x8, #0x19e8]
1004c2464:      cbz x8, 0x1004c24bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7d0>
1004c2468:      ldr x9, [x8]
1004c246c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c2470:      cmp x9, x10
1004c2474:      b.hs    0x1004c27e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c2478:      add x10, x9, #0x1
1004c247c:      str x10, [x8]
1004c2480:      ldr x10, [x8, #0x18]
1004c2484:      cmp x23, x10
1004c2488:      b.hs    0x1004c27f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c248c:      ldr x10, [x8, #0x10]
1004c2490:      mov w11, #0x18              ; =24
1004c2494:      madd    x10, x23, x11, x10
1004c2498:      ldr x11, [x10]
1004c249c:      cmp x11, #0x1
1004c24a0:      b.ne    0x1004c27f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c24a4:      ldr x23, [x10, #0x8]
1004c24a8:      str x9, [x8]
1004c24ac:      ldr x8, [x25, #0x48]
1004c24b0:      cmn x8, #0x1
1004c24b4:      b.ne    0x1004c24d4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x7e8>
1004c24b8:      b   0x1004c2534 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c24bc:      mov x0, x23
1004c24c0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c24c4:      mov x23, x0
1004c24c8:      ldr x8, [x25, #0x48]
1004c24cc:      cmn x8, #0x1
1004c24d0:      b.eq    0x1004c2534 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c24d4:      mrs x9, TPIDRRO_EL0
1004c24d8:      and x9, x9, #0xfffffffffffffff8
1004c24dc:      ldr x8, [x9, x8, lsl #3]
1004c24e0:      cbz x8, 0x1004c2534 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c24e4:      ldr x8, [x8, #0x19e8]
1004c24e8:      cbz x8, 0x1004c2534 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x848>
1004c24ec:      ldr x9, [x8]
1004c24f0:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c24f4:      cmp x9, x10
1004c24f8:      b.hs    0x1004c27e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xaf8>
1004c24fc:      add x10, x9, #0x1
1004c2500:      str x10, [x8]
1004c2504:      ldr x10, [x8, #0x18]
1004c2508:      cmp x22, x10
1004c250c:      b.hs    0x1004c27f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb04>
1004c2510:      ldr x10, [x8, #0x10]
1004c2514:      mov w11, #0x18              ; =24
1004c2518:      madd    x10, x22, x11, x10
1004c251c:      ldr x11, [x10]
1004c2520:      cmp x11, #0x1
1004c2524:      b.ne    0x1004c27f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb08>
1004c2528:      ldr x2, [x10, #0x8]
1004c252c:      str x9, [x8]
1004c2530:      b   0x1004c2540 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x854>
1004c2534:      mov x0, x22
1004c2538:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c253c:      mov x2, x0
1004c2540:      lsr x1, x24, #32
1004c2544:      mov x0, x23
1004c2548:      mov x3, x20
1004c254c:      bl  0x10059b7fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes34try_birth_stamp_preinstalled_shape>
1004c2550:      tbz w0, #0x0, 0x1004c2784 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa98>
1004c2554:      cbz x23, 0x1004c2564 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x878>
1004c2558:      ldurh   w8, [x23, #-0x6]
1004c255c:      orr w8, w8, #0x200
1004c2560:      sturh   w8, [x23, #-0x6]
1004c2564:      cbz x20, 0x1004c2614 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x928>
1004c2568:      lsl x8, x20, #4
1004c256c:      sub x9, x8, #0x10
1004c2570:      cmp x9, #0x80
1004c2574:      b.hs    0x1004c2584 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x898>
1004c2578:      mov x10, #0x0               ; =0
1004c257c:      mov x9, x19
1004c2580:      b   0x1004c25f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x908>
1004c2584:      lsr x9, x9, #4
1004c2588:      add x9, x9, #0x1
1004c258c:      ands    x10, x9, #0x7
1004c2590:      mov w11, #0x8               ; =8
1004c2594:      csel    x10, x11, x10, eq
1004c2598:      sub x10, x9, x10
1004c259c:      add x9, x19, x10, lsl #4
1004c25a0:      add x11, x19, #0x48
1004c25a4:      add x12, x23, #0x20
1004c25a8:      mov x13, x10
1004c25ac:      sub x14, x11, #0x30
1004c25b0:      ldur    q0, [x11, #-0x40]
1004c25b4:      ld1.d   { v0 }[1], [x14]
1004c25b8:      sub x14, x11, #0x10
1004c25bc:      ldur    q1, [x11, #-0x20]
1004c25c0:      ld1.d   { v1 }[1], [x14]
1004c25c4:      add x14, x11, #0x10
1004c25c8:      ldr q2, [x11]
1004c25cc:      ld1.d   { v2 }[1], [x14]
1004c25d0:      add x14, x11, #0x30
1004c25d4:      ldr q3, [x11, #0x20]
1004c25d8:      ld1.d   { v3 }[1], [x14]
1004c25dc:      stp q0, q1, [x12, #-0x10]
1004c25e0:      stp q2, q3, [x12, #0x10]
1004c25e4:      add x11, x11, #0x80
1004c25e8:      add x12, x12, #0x40
1004c25ec:      subs    x13, x13, #0x8
1004c25f0:      b.ne    0x1004c25ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x8c0>
1004c25f4:      add x8, x19, x8
1004c25f8:      add x10, x23, x10, lsl #3
1004c25fc:      add x10, x10, #0x10
1004c2600:      ldr x11, [x9, #0x8]
1004c2604:      add x9, x9, #0x10
1004c2608:      str x11, [x10], #0x8
1004c260c:      cmp x9, x8
1004c2610:      b.ne    0x1004c2600 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x914>
1004c2614:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c2618:      add x0, x0, #0xb60
1004c261c:      ldr x8, [x0]
1004c2620:      blr x8
1004c2624:      ldrb    w8, [x0, #0x38]
1004c2628:      cmp w8, #0x1
1004c262c:      b.ne    0x1004c279c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab0>
1004c2630:      ldr x8, [x0]
1004c2634:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004c2638:      cmp x8, x9
1004c263c:      b.hs    0x1004c27b0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xac4>
1004c2640:      ldr x8, [x0, #0x20]
1004c2644:      cmp x8, #0x1, lsl #12       ; =0x1000
1004c2648:      b.hi    0x1004c27bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xad0>
1004c264c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2650:      add x8, x8, #0x610
1004c2654:      ldapr   x8, [x8]
1004c2658:      cbnz    x8, 0x1004c27d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xae4>
1004c265c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2660:      ldrb    w8, [x8, #0x618]
1004c2664:      cbz w8, 0x1004c2694 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2668:      bl  0x1004e043c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5arena4walk18arena_in_use_bytes>
1004c266c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2670:      add x8, x8, #0xdb0
1004c2674:      ldapr   x8, [x8]
1004c2678:      cbnz    x8, 0x1004c2808 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb1c>
1004c267c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2680:      ldr x8, [x8, #0xdb8]
1004c2684:      cmp x0, x8
1004c2688:      b.lo    0x1004c2694 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c268c:      mov w8, #0x1                ; =1
1004c2690:      strb    w8, [x21]
1004c2694:      mov x19, #0x7ffd000000000000 ; =9222527611924643840
1004c2698:      bfxil   x19, x23, #0, #48
1004c269c:      add x0, sp, #0x70
1004c26a0:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c26a4:      mov w0, #0x1                ; =1
1004c26a8:      b   0x1004c2720 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa34>
1004c26ac:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c26b0:      add x0, x0, #0xda8
1004c26b4:      ldr x8, [x0]
1004c26b8:      blr x8
1004c26bc:      ldr x8, [x0]
1004c26c0:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c26c4:      add x0, x0, #0x808
1004c26c8:      ldr x9, [x0]
1004c26cc:      blr x9
1004c26d0:      ldr x9, [x0]
1004c26d4:      cmp x9, x8
1004c26d8:      b.ls    0x1004c26f8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa0c>
1004c26dc:      str x8, [x0]
1004c26e0:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004c26e4:      add x0, x0, #0x778
1004c26e8:      ldr x8, [x0]
1004c26ec:      blr x8
1004c26f0:      mov w8, #0x1                ; =1
1004c26f4:      strb    w8, [x0]
1004c26f8:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004c26fc:      b   0x1004c2384 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x698>
1004c2700:      sub x9, x10, #0x10
1004c2704:      ldr x10, [x9, x8]
1004c2708:      cmp x10, x20
1004c270c:      b.eq    0x1004c2838 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb4c>
1004c2710:      subs    x8, x8, #0x20
1004c2714:      b.ne    0x1004c2704 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xa18>
1004c2718:      mov x0, #0x0                ; =0
1004c271c:      str x28, [x21]
1004c2720:      mov x1, x19
1004c2724:      ldp x29, x30, [sp, #0xe0]
1004c2728:      ldp x20, x19, [sp, #0xd0]
1004c272c:      ldp x22, x21, [sp, #0xc0]
1004c2730:      ldp x24, x23, [sp, #0xb0]
1004c2734:      ldp x26, x25, [sp, #0xa0]
1004c2738:      ldp x28, x27, [sp, #0x90]
1004c273c:      add sp, sp, #0xf0
1004c2740:      ret
1004c2744:      cmp w8, #0x1
1004c2748:      b.ne    0x1004c27a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xab8>
1004c274c:      adrp    x1, 0x10018b000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs7wa3bwJW6LN_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x34>
1004c2750:      add x1, x1, #0xa0
1004c2754:      mov x0, x21
1004c2758:      bl  0x1009be91c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1004c275c:      strb    wzr, [x21, #0x20]
1004c2760:      ldr x28, [x21]
1004c2764:      mov x8, #0x7fffffffffffffff ; =9223372036854775807
1004c2768:      cmp x28, x8
1004c276c:      b.lo    0x1004c1d3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x50>
1004c2770:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c2774:      add x0, x0, #0x3f8
1004c2778:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c277c:      tbnz    w24, #0x8, 0x1004c2804 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0xb18>
1004c2780:      bl  0x100ae1934 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes24shape_id_exhausted_abort>
1004c2784:      adrp    x0, 0x100c0a000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime10bun_compat12width_tables8EAW_WIDE+0x6420>
1004c2788:      add x0, x0, #0x834
1004c278c:      adrp    x2, 0x100e8d000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x1ef0>
1004c2790:      add x2, x2, #0xfd0
1004c2794:      mov w1, #0x75               ; =117
1004c2798:      bl  0x100ab0e68 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
1004c279c:      bl  0x100abc4b0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004c27a0:      cbnz    x0, 0x1004c2630 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x944>
1004c27a4:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004c27a8:      add x0, x0, #0x850
1004c27ac:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004c27b0:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004c27b4:      add x0, x0, #0x3c8
1004c27b8:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c27bc:      bl  0x100addfc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004c27c0:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c27c4:      add x8, x8, #0x610
1004c27c8:      ldapr   x8, [x8]
1004c27cc:      cbz x8, 0x1004c265c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x970>
1004c27d0:      bl  0x100ab778c <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtCs7wa3bwJW6LN_13perry_runtime2gc14gen_gc_enabled0E0zEB1y_>
1004c27d4:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c27d8:      ldrb    w8, [x8, #0x618]
1004c27dc:      cbnz    w8, 0x1004c2668 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x97c>
1004c27e0:      b   0x1004c2694 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c27e4:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c27e8:      add x0, x0, #0x8a8
1004c27ec:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c27f0:      bl  0x100ae2488 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c27f4:      adrp    x0, 0x100bd8000 <_PERRY_EMPTY_STRING+0x2f0>
1004c27f8:      add x0, x0, #0x96c
1004c27fc:      mov w1, #0xb                ; =11
1004c2800:      bl  0x100ae2450 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c2804:      bl  0x100ae1950 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object6shapes25invalid_shape_facts_abort>
1004c2808:      mov x19, x0
1004c280c:      bl  0x100ab8244 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockjE10initializeNCINvB2_11get_or_initNCNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc11heap_budget38gc_tiny_parse_in_use_trigger_dyn_bytes0E0zEB1A_>
1004c2810:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c2814:      ldr x8, [x8, #0xdb8]
1004c2818:      cmp x19, x8
1004c281c:      b.hs    0x1004c268c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a0>
1004c2820:      b   0x1004c2694 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json19parse_inline_object8allocate+0x9a8>
1004c2824:      adrp    x2, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c2828:      add x2, x2, #0xe58
1004c282c:      mov w0, #0x5                ; =5
1004c2830:      mov w1, #0x5                ; =5
1004c2834:      bl  0x100ab0d0c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1004c2838:      adrp    x3, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c283c:      add x3, x3, #0xe70
1004c2840:      mov x0, #0x0                ; =0
1004c2844:      mov x1, x20
1004c2848:      mov w2, #0x8                ; =8
1004c284c:      bl  0x100ab0ecc <__RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail>
1004c2850:      nop
1004c2854:      nop
1004c2858:      nop
1004c285c:      nop
1004c2860:      nop
1004c2864:      nop
1004c2868:      nop
1004c286c:      nop
1004c2870:      nop
1004c2874:      nop
1004c2878:      nop
1004c287c:      nop
