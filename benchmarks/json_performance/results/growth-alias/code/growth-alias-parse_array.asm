/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001003b9c64 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array>:
1003b9c64:      sub sp, sp, #0x30
1003b9c68:      stp x20, x19, [sp, #0x10]
1003b9c6c:      stp x29, x30, [sp, #0x20]
1003b9c70:      add x29, sp, #0x20
1003b9c74:      mov x19, x0
1003b9c78:      ldp x8, x9, [x0, #0x30]
1003b9c7c:      add x20, x9, #0x1
1003b9c80:      str x20, [x0, #0x38]
1003b9c84:      cmp x20, x8
1003b9c88:      b.hs    0x1003b9cec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
1003b9c8c:      ldr x9, [x19, #0x28]
1003b9c90:      mov x10, #0x2600            ; =9728
1003b9c94:      movk    x10, #0x1, lsl #32
1003b9c98:      ldrb    w11, [x9, x20]
1003b9c9c:      cmp w11, #0x20
1003b9ca0:      b.hi    0x1003b9cec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
1003b9ca4:      lsr x11, x10, x11
1003b9ca8:      tbz w11, #0x0, 0x1003b9cec <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x88>
1003b9cac:      add x20, x20, #0x1
1003b9cb0:      str x20, [x19, #0x38]
1003b9cb4:      cmp x8, x20
1003b9cb8:      b.ne    0x1003b9c98 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x34>
1003b9cbc:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003b9cc0:      add x0, x0, #0x8
1003b9cc4:      ldr x8, [x0]
1003b9cc8:      blr x8
1003b9ccc:      ldrb    w8, [x0, #0x20]
1003b9cd0:      cbnz    w8, 0x1003b9dc4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x160>
1003b9cd4:      ldr x8, [x0]
1003b9cd8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003b9cdc:      cmp x8, x9
1003b9ce0:      b.hs    0x1003b9df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
1003b9ce4:      ldr x1, [x0, #0x18]
1003b9ce8:      b   0x1003b9d7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1003b9cec:      adrp    x0, 0x101126000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime4json25OBJECT_PROTO_TOJSON_STATE0s_023___RUST_STD_INTERNAL_VAL+0x10>
1003b9cf0:      add x0, x0, #0x8
1003b9cf4:      ldr x9, [x0]
1003b9cf8:      blr x9
1003b9cfc:      ldrb    w9, [x0, #0x20]
1003b9d00:      cbnz    w9, 0x1003b9d90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x12c>
1003b9d04:      ldr x9, [x0]
1003b9d08:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1003b9d0c:      cmp x9, x10
1003b9d10:      b.hs    0x1003b9df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
1003b9d14:      ldr x1, [x0, #0x18]
1003b9d18:      subs    x8, x8, x20
1003b9d1c:      b.ls    0x1003b9d7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1003b9d20:      ldr x9, [x19, #0x28]
1003b9d24:      ldrb    w9, [x9, x20]
1003b9d28:      cmp w9, #0x7b
1003b9d2c:      b.ne    0x1003b9d7c <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x118>
1003b9d30:      mov x9, #-0x5555555555555556 ; =-6148914691236517206
1003b9d34:      movk    x9, #0xaaab
1003b9d38:      umulh   x8, x8, x9
1003b9d3c:      lsr x8, x8, #6
1003b9d40:      mov w9, #0x10               ; =16
1003b9d44:      cmp x8, #0x10
1003b9d48:      csel    x8, x8, x9, hi
1003b9d4c:      mov w9, #0x4000             ; =16384
1003b9d50:      cmp x8, #0x4, lsl #12       ; =0x4000
1003b9d54:      csel    x0, x8, x9, lo
1003b9d58:      mov x20, x1
1003b9d5c:      bl  0x10042fcac <_js_array_alloc>
1003b9d60:      mov x1, x0
1003b9d64:      mov x0, x19
1003b9d68:      mov x2, x20
1003b9d6c:      ldp x29, x30, [sp, #0x20]
1003b9d70:      ldp x20, x19, [sp, #0x10]
1003b9d74:      add sp, sp, #0x30
1003b9d78:      b   0x1003ba384 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser16parse_array_tail>
1003b9d7c:      mov x0, x19
1003b9d80:      ldp x29, x30, [sp, #0x20]
1003b9d84:      ldp x20, x19, [sp, #0x10]
1003b9d88:      add sp, sp, #0x30
1003b9d8c:      b   0x1003bab90 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser18parse_array_prefix>
1003b9d90:      cmp w9, #0x2
1003b9d94:      b.eq    0x1003b9e00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
1003b9d98:      stp x8, x0, [sp]
1003b9d9c:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003b9da0:      add x1, x1, #0x824
1003b9da4:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003b9da8:      ldp x8, x0, [sp]
1003b9dac:      strb    wzr, [x0, #0x20]
1003b9db0:      ldr x9, [x0]
1003b9db4:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1003b9db8:      cmp x9, x10
1003b9dbc:      b.lo    0x1003b9d14 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0xb0>
1003b9dc0:      b   0x1003b9df4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x190>
1003b9dc4:      cmp w8, #0x1
1003b9dc8:      b.ne    0x1003b9e00 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x19c>
1003b9dcc:      adrp    x1, 0x1007c8000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x468>
1003b9dd0:      add x1, x1, #0x824
1003b9dd4:      str x0, [sp, #0x8]
1003b9dd8:      bl  0x100b9b2dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1003b9ddc:      ldr x0, [sp, #0x8]
1003b9de0:      strb    wzr, [x0, #0x20]
1003b9de4:      ldr x8, [x0]
1003b9de8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1003b9dec:      cmp x8, x9
1003b9df0:      b.lo    0x1003b9ce4 <__RNvMs_NtNtCs5gMwpk3Cs4e_13perry_runtime4json6parserNtB4_12DirectParser11parse_array+0x80>
1003b9df4:      adrp    x0, 0x101090000 <_anon.438b28c8644b10f28676d307896bf03a.21>
1003b9df8:      add x0, x0, #0xe70
1003b9dfc:      bl  0x100c8d1dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1003b9e00:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1003b9e04:      add x0, x0, #0xed8
1003b9e08:      bl  0x100ccf55c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
