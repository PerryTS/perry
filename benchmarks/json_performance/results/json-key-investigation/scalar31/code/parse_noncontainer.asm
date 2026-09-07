/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/scalar31-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004d1cb4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>:
1004d1cb4:      sub sp, sp, #0x60
1004d1cb8:      stp x24, x23, [sp, #0x20]
1004d1cbc:      stp x22, x21, [sp, #0x30]
1004d1cc0:      stp x20, x19, [sp, #0x40]
1004d1cc4:      stp x29, x30, [sp, #0x50]
1004d1cc8:      add x29, sp, #0x50
1004d1ccc:      ldrb    w9, [x0, #0x14]
1004d1cd0:      orr w8, w9, #0x20
1004d1cd4:      cmp w8, #0x7b
1004d1cd8:      b.ne    0x1004d1cf4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x40>
1004d1cdc:      ldp x29, x30, [sp, #0x50]
1004d1ce0:      ldp x20, x19, [sp, #0x40]
1004d1ce4:      ldp x22, x21, [sp, #0x30]
1004d1ce8:      ldp x24, x23, [sp, #0x20]
1004d1cec:      add sp, sp, #0x60
1004d1cf0:      b   0x1004cf778 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1004d1cf4:      mov x21, #0x0               ; =0
1004d1cf8:      sub x10, x1, #0x1
1004d1cfc:      mov x8, #0x2600             ; =9728
1004d1d00:      movk    x8, #0x1, lsl #32
1004d1d04:      cmp w9, #0x20
1004d1d08:      b.hi    0x1004d1d34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x80>
1004d1d0c:      mov w11, w9
1004d1d10:      lsr x11, x8, x11
1004d1d14:      tbz w11, #0x0, 0x1004d1d34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x80>
1004d1d18:      cmp x10, x21
1004d1d1c:      b.eq    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1d20:      add x9, x0, x21
1004d1d24:      ldrb    w9, [x9, #0x15]
1004d1d28:      add x21, x21, #0x1
1004d1d2c:      cmp w9, #0x20
1004d1d30:      b.ls    0x1004d1d0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x58>
1004d1d34:      add x8, x0, #0x13
1004d1d38:      add x11, x0, x21
1004d1d3c:      mov x14, #0x2600            ; =9728
1004d1d40:      movk    x14, #0x1, lsl #32
1004d1d44:      mov x12, x21
1004d1d48:      ldrb    w13, [x8, x1]
1004d1d4c:      cmp w13, #0x20
1004d1d50:      b.hi    0x1004d1d70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d1d54:      lsr x15, x14, x13
1004d1d58:      tbz w15, #0x0, 0x1004d1d70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d1d5c:      sub x8, x8, #0x1
1004d1d60:      add x12, x12, #0x1
1004d1d64:      cmp x1, x12
1004d1d68:      b.ne    0x1004d1d48 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x94>
1004d1d6c:      b   0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1d70:      sub x8, x1, x12
1004d1d74:      cmp x8, #0x4
1004d1d78:      b.eq    0x1004d1de4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x130>
1004d1d7c:      cmp x8, #0x5
1004d1d80:      b.ne    0x1004d1dec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x138>
1004d1d84:      cmp w9, #0x22
1004d1d88:      b.eq    0x1004d1ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x22c>
1004d1d8c:      cmp w9, #0x2d
1004d1d90:      b.eq    0x1004d1e08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x154>
1004d1d94:      cmp w9, #0x66
1004d1d98:      b.ne    0x1004d1dfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x148>
1004d1d9c:      add x8, x0, x21
1004d1da0:      ldrb    w9, [x8, #0x15]
1004d1da4:      cmp w9, #0x61
1004d1da8:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1dac:      ldrb    w8, [x8, #0x16]
1004d1db0:      cmp w8, #0x6c
1004d1db4:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1db8:      add x8, x0, x21
1004d1dbc:      ldrb    w9, [x8, #0x17]
1004d1dc0:      cmp w9, #0x73
1004d1dc4:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1dc8:      ldrb    w8, [x8, #0x18]
1004d1dcc:      cmp w8, #0x65
1004d1dd0:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1dd4:      mov x8, #0x2                ; =2
1004d1dd8:      movk    x8, #0x7ffc, lsl #48
1004d1ddc:      orr x19, x8, #0x1
1004d1de0:      b   0x1004d1e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d1de4:      cmp w9, #0x6d
1004d1de8:      b.gt    0x1004d1f58 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2a4>
1004d1dec:      cmp w9, #0x22
1004d1df0:      b.eq    0x1004d1ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x22c>
1004d1df4:      cmp w9, #0x2d
1004d1df8:      b.eq    0x1004d1e08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x154>
1004d1dfc:      sub w9, w9, #0x30
1004d1e00:      cmp w9, #0xa
1004d1e04:      b.hs    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1e08:      mov x19, x0
1004d1e0c:      add x0, x11, #0x14
1004d1e10:      mov x20, x1
1004d1e14:      mov x1, x8
1004d1e18:      bl  0x1004bc148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004d1e1c:      mov x8, x0
1004d1e20:      mov x0, x19
1004d1e24:      mov x19, x1
1004d1e28:      mov x1, x20
1004d1e2c:      cmp x8, #0x1
1004d1e30:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1e34:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d1e38:      add x0, x0, #0x9d0
1004d1e3c:      ldr x8, [x0]
1004d1e40:      blr x8
1004d1e44:      ldrb    w9, [x0]
1004d1e48:      strb    wzr, [x0]
1004d1e4c:      cbz w9, 0x1004d1e8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1d8>
1004d1e50:      mov x8, x0
1004d1e54:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d1e58:      add x0, x0, #0x9e8
1004d1e5c:      ldr x9, [x0]
1004d1e60:      blr x9
1004d1e64:      ldrb    w9, [x0]
1004d1e68:      tst w9, #0x3
1004d1e6c:      b.ne    0x1004d1e84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1d0>
1004d1e70:      adrp    x9, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004d1e74:      add x9, x9, #0x8fc
1004d1e78:      ldapr   w9, [x9]
1004d1e7c:      cmp w9, #0x0
1004d1e80:      b.le    0x1004d200c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x358>
1004d1e84:      mov w9, #0x1                ; =1
1004d1e88:      strb    w9, [x8]
1004d1e8c:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004d1e90:      add x0, x0, #0xb60
1004d1e94:      ldr x8, [x0]
1004d1e98:      blr x8
1004d1e9c:      ldrb    w8, [x0, #0x38]
1004d1ea0:      cmp w8, #0x1
1004d1ea4:      b.ne    0x1004d20dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x428>
1004d1ea8:      ldr x8, [x0]
1004d1eac:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004d1eb0:      cmp x8, x9
1004d1eb4:      b.hs    0x1004d20f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x43c>
1004d1eb8:      ldr x8, [x0, #0x20]
1004d1ebc:      cmp x8, #0x1, lsl #12       ; =0x1000
1004d1ec0:      b.hi    0x1004d20fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x448>
1004d1ec4:      mov x0, x19
1004d1ec8:      ldp x29, x30, [sp, #0x50]
1004d1ecc:      ldp x20, x19, [sp, #0x40]
1004d1ed0:      ldp x22, x21, [sp, #0x30]
1004d1ed4:      ldp x24, x23, [sp, #0x20]
1004d1ed8:      add sp, sp, #0x60
1004d1edc:      ret
1004d1ee0:      cmp x10, x12
1004d1ee4:      b.eq    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1ee8:      cmp x8, #0x7
1004d1eec:      b.hi    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1ef0:      cmp w13, #0x22
1004d1ef4:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1ef8:      sub x20, x8, #0x2
1004d1efc:      sub x8, x1, #0x2
1004d1f00:      add x9, x0, x21
1004d1f04:      add x19, x9, #0x15
1004d1f08:      cmp x8, x12
1004d1f0c:      b.ne    0x1004d1fdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x328>
1004d1f10:      add x8, sp, #0x8
1004d1f14:      mov x22, x0
1004d1f18:      mov x0, x19
1004d1f1c:      mov x23, x1
1004d1f20:      mov x1, x20
1004d1f24:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004d1f28:      mov x1, x23
1004d1f2c:      mov x0, x22
1004d1f30:      ldr x8, [sp, #0x8]
1004d1f34:      cbnz    x8, 0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1f38:      cmp x20, #0x2
1004d1f3c:      b.gt    0x1004d2060 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3ac>
1004d1f40:      cbz x20, 0x1004d207c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3c8>
1004d1f44:      cmp x20, #0x1
1004d1f48:      b.ne    0x1004d20a4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3f0>
1004d1f4c:      ldrb    w8, [x19]
1004d1f50:      mov x9, #0x10000000000      ; =1099511627776
1004d1f54:      b   0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x41c>
1004d1f58:      cmp w9, #0x74
1004d1f5c:      b.eq    0x1004d1fa0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2ec>
1004d1f60:      cmp w9, #0x6e
1004d1f64:      b.ne    0x1004d1dfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x148>
1004d1f68:      add x8, x0, x21
1004d1f6c:      ldrb    w9, [x8, #0x15]
1004d1f70:      cmp w9, #0x75
1004d1f74:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1f78:      ldrb    w8, [x8, #0x16]
1004d1f7c:      cmp w8, #0x6c
1004d1f80:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1f84:      add x8, x0, x21
1004d1f88:      ldrb    w8, [x8, #0x17]
1004d1f8c:      cmp w8, #0x6c
1004d1f90:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1f94:      mov x19, #0x2               ; =2
1004d1f98:      movk    x19, #0x7ffc, lsl #48
1004d1f9c:      b   0x1004d1e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d1fa0:      add x8, x0, x21
1004d1fa4:      ldrb    w9, [x8, #0x15]
1004d1fa8:      cmp w9, #0x72
1004d1fac:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1fb0:      ldrb    w8, [x8, #0x16]
1004d1fb4:      cmp w8, #0x75
1004d1fb8:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1fbc:      add x8, x0, x21
1004d1fc0:      ldrb    w8, [x8, #0x17]
1004d1fc4:      cmp w8, #0x65
1004d1fc8:      b.ne    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1fcc:      mov x8, #0x2                ; =2
1004d1fd0:      movk    x8, #0x7ffc, lsl #48
1004d1fd4:      add x19, x8, #0x2
1004d1fd8:      b   0x1004d1e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d1fdc:      mov x8, x20
1004d1fe0:      mov x9, x19
1004d1fe4:      ldrb    w10, [x9], #0x1
1004d1fe8:      cmp w10, #0x20
1004d1fec:      b.lo    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1ff0:      cmp w10, #0x22
1004d1ff4:      b.eq    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d1ff8:      cmp w10, #0x5c
1004d1ffc:      b.eq    0x1004d1cdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x28>
1004d2000:      subs    x8, x8, #0x1
1004d2004:      b.ne    0x1004d1fe4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x330>
1004d2008:      b   0x1004d1f10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x25c>
1004d200c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2010:      add x0, x0, #0xda8
1004d2014:      ldr x8, [x0]
1004d2018:      blr x8
1004d201c:      ldr x8, [x0]
1004d2020:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2024:      add x0, x0, #0x808
1004d2028:      ldr x9, [x0]
1004d202c:      blr x9
1004d2030:      ldr x9, [x0]
1004d2034:      cmp x9, x8
1004d2038:      b.ls    0x1004d2058 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3a4>
1004d203c:      str x8, [x0]
1004d2040:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2044:      add x0, x0, #0x778
1004d2048:      ldr x8, [x0]
1004d204c:      blr x8
1004d2050:      mov w8, #0x1                ; =1
1004d2054:      strb    w8, [x0]
1004d2058:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004d205c:      b   0x1004d1e8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1d8>
1004d2060:      cmp x20, #0x3
1004d2064:      b.eq    0x1004d2084 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3d0>
1004d2068:      cmp x20, #0x4
1004d206c:      b.ne    0x1004d20bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x408>
1004d2070:      ldr w8, [x19]
1004d2074:      mov x9, #0x40000000000      ; =4398046511104
1004d2078:      b   0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x41c>
1004d207c:      mov x19, #0x7ff9000000000000 ; =9221401712017801216
1004d2080:      b   0x1004d1e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d2084:      ldrb    w8, [x19]
1004d2088:      add x9, x0, x21
1004d208c:      ldrb    w10, [x9, #0x16]
1004d2090:      ldrb    w9, [x9, #0x17]
1004d2094:      orr x8, x8, x10, lsl #8
1004d2098:      orr x8, x8, x9, lsl #16
1004d209c:      mov x9, #0x30000000000      ; =3298534883328
1004d20a0:      b   0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x41c>
1004d20a4:      ldrb    w8, [x19]
1004d20a8:      add x9, x0, x21
1004d20ac:      ldrb    w9, [x9, #0x16]
1004d20b0:      orr x8, x8, x9, lsl #8
1004d20b4:      mov x9, #0x20000000000      ; =2199023255552
1004d20b8:      b   0x1004d20d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x41c>
1004d20bc:      ldr w8, [x19]
1004d20c0:      add x9, x0, x21
1004d20c4:      ldrb    w9, [x9, #0x19]
1004d20c8:      orr x8, x8, x9, lsl #32
1004d20cc:      mov x9, #0x50000000000      ; =5497558138880
1004d20d0:      movk    x9, #0x7ff9, lsl #48
1004d20d4:      orr x19, x8, x9
1004d20d8:      b   0x1004d1e34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d20dc:      bl  0x100abc4b0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004d20e0:      cbnz    x0, 0x1004d1ea8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1f4>
1004d20e4:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004d20e8:      add x0, x0, #0x850
1004d20ec:      bl  0x100aeefdc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004d20f0:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004d20f4:      add x0, x0, #0x3c8
1004d20f8:      bl  0x100ab0bdc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004d20fc:      bl  0x100addfc8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004d2100:      b   0x1004d1ec4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x210>
