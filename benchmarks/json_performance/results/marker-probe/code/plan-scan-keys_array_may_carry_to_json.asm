/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002f0aa0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json>:
1002f0aa0:      stp x29, x30, [sp, #-0x10]!
1002f0aa4:      mov x29, sp
1002f0aa8:      sub x8, x0, #0x1, lsl #12   ; =0x1000
1002f0aac:      tst x0, #0x7
1002f0ab0:      mov w9, #0x100000           ; =1048576
1002f0ab4:      ccmp    x0, x9, #0x0, eq
1002f0ab8:      mov x9, #0x7fffffff0000     ; =140737488289792
1002f0abc:      movk    x9, #0xefff
1002f0ac0:      ccmp    x8, x9, #0x2, hs
1002f0ac4:      b.hi    0x1002f0c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d4>
1002f0ac8:      ldurb   w8, [x0, #-0x8]
1002f0acc:      cmp w8, #0x1
1002f0ad0:      b.ne    0x1002f0c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d4>
1002f0ad4:      mov x9, x0
1002f0ad8:      ldr w8, [x0]
1002f0adc:      mov w0, #0x1                ; =1
1002f0ae0:      cmp w8, #0x1, lsl #12       ; =0x1000
1002f0ae4:      b.hi    0x1002f0c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d8>
1002f0ae8:      ldr w10, [x9, #0x4]
1002f0aec:      cmp w8, w10
1002f0af0:      b.hi    0x1002f0c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d8>
1002f0af4:      cbz w8, 0x1002f0c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e0>
1002f0af8:      add x9, x9, #0x8
1002f0afc:      mov x10, #0x5f5f            ; =24415
1002f0b00:      movk    x10, #0x6570, lsl #16
1002f0b04:      movk    x10, #0x7272, lsl #32
1002f0b08:      movk    x10, #0x5f79, lsl #48
1002f0b0c:      mov x11, #0x7ff9000000000000 ; =9221401712017801216
1002f0b10:      mov x12, #0xfa0000000000    ; =274877906944000
1002f0b14:      movk    x12, #0xffff, lsl #48
1002f0b18:      mov x13, #0x7fff000000000000 ; =9223090561878065152
1002f0b1c:      mov w14, #0x6f74            ; =28532
1002f0b20:      movk    w14, #0x534a, lsl #16
1002f0b24:      mov w15, #0x4e4f            ; =20047
1002f0b28:      mov x16, #0x5f5f            ; =24415
1002f0b2c:      movk    x16, #0x6f6d, lsl #16
1002f0b30:      movk    x16, #0x7564, lsl #32
1002f0b34:      movk    x16, #0x656c, lsl #48
1002f0b38:      mov w17, #0x5f5f            ; =24415
1002f0b3c:      mov x0, #0x6566             ; =25958
1002f0b40:      movk    x0, #0x6374, lsl #16
1002f0b44:      movk    x0, #0x5f68, lsl #32
1002f0b48:      movk    x0, #0x6168, lsl #48
1002f0b4c:      mov x1, #0x6168             ; =24936
1002f0b50:      movk    x1, #0x646e, lsl #16
1002f0b54:      movk    x1, #0x656c, lsl #32
1002f0b58:      movk    x1, #0x5f5f, lsl #48
1002f0b5c:      mov x2, #0x6574             ; =25972
1002f0b60:      movk    x2, #0x706d, lsl #16
1002f0b64:      movk    x2, #0x726f, lsl #32
1002f0b68:      movk    x2, #0x6c61, lsl #48
1002f0b6c:      mov x3, #0x5f6c             ; =24428
1002f0b70:      movk    x3, #0x6563, lsl #16
1002f0b74:      movk    x3, #0x6c6c, lsl #32
1002f0b78:      movk    x3, #0x5f5f, lsl #48
1002f0b7c:      b   0x1002f0ba4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x104>
1002f0b80:      ldur    x5, [x4, #0x14]
1002f0b84:      ldur    x6, [x4, #0x1c]
1002f0b88:      ldur    x4, [x4, #0x22]
1002f0b8c:      cmp x5, x10
1002f0b90:      ccmp    x6, x0, #0x0, eq
1002f0b94:      ccmp    x4, x1, #0x0, eq
1002f0b98:      b.eq    0x1002f0c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d4>
1002f0b9c:      subs    x8, x8, #0x1
1002f0ba0:      b.eq    0x1002f0c80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e0>
1002f0ba4:      ldr x4, [x9], #0x8
1002f0ba8:      and x5, x4, #0xffff000000000000
1002f0bac:      cmp x5, x11
1002f0bb0:      b.eq    0x1002f0bf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x158>
1002f0bb4:      cmp x5, x13
1002f0bb8:      and x4, x4, #0xffffffffffff
1002f0bbc:      ccmp    x4, #0x0, #0x4, eq
1002f0bc0:      b.eq    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0bc4:      ldr w5, [x4, #0x4]
1002f0bc8:      cmp w5, #0x15
1002f0bcc:      b.gt    0x1002f0c30 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x190>
1002f0bd0:      cmp w5, #0x6
1002f0bd4:      b.eq    0x1002f0c60 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1c0>
1002f0bd8:      cmp w5, #0xa
1002f0bdc:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0be0:      ldur    x5, [x4, #0x14]
1002f0be4:      ldrh    w4, [x4, #0x1c]
1002f0be8:      cmp x5, x16
1002f0bec:      ccmp    w4, w17, #0x0, eq
1002f0bf0:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0bf4:      b   0x1002f0c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d4>
1002f0bf8:      and x4, x4, #0xff0000000000
1002f0bfc:      add x4, x4, x12
1002f0c00:      lsr x4, x4, #40
1002f0c04:      cmp x4, #0xf
1002f0c08:      b.gt    0x1002f0c1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x17c>
1002f0c0c:      cbz x4, 0x1002f0c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e8>
1002f0c10:      cmp x4, #0x4
1002f0c14:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0c18:      b   0x1002f0c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e8>
1002f0c1c:      cmp x4, #0x10
1002f0c20:      b.eq    0x1002f0c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e8>
1002f0c24:      cmp x4, #0x11
1002f0c28:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0c2c:      b   0x1002f0c88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1e8>
1002f0c30:      cmp w5, #0x16
1002f0c34:      b.eq    0x1002f0b80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xe0>
1002f0c38:      cmp w5, #0x17
1002f0c3c:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0c40:      ldur    x5, [x4, #0x14]
1002f0c44:      ldur    x6, [x4, #0x1c]
1002f0c48:      ldur    x4, [x4, #0x23]
1002f0c4c:      cmp x5, x10
1002f0c50:      ccmp    x6, x2, #0x0, eq
1002f0c54:      ccmp    x4, x3, #0x0, eq
1002f0c58:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0c5c:      b   0x1002f0c74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d4>
1002f0c60:      ldr w5, [x4, #0x14]
1002f0c64:      ldrh    w4, [x4, #0x18]
1002f0c68:      cmp w5, w14
1002f0c6c:      ccmp    w4, w15, #0x0, eq
1002f0c70:      b.ne    0x1002f0b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0xfc>
1002f0c74:      mov w0, #0x1                ; =1
1002f0c78:      ldp x29, x30, [sp], #0x10
1002f0c7c:      ret
1002f0c80:      mov w0, #0x0                ; =0
1002f0c84:      b   0x1002f0c78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe28keys_array_may_carry_to_json+0x1d8>
1002f0c88:      adrp    x2, 0x101094000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6object23HTTP_METHODS_CACHE_SLOT+0x10>
1002f0c8c:      add x2, x2, #0x1b8
1002f0c90:      mov w0, #0x5                ; =5
1002f0c94:      mov w1, #0x5                ; =5
1002f0c98:      bl  0x100c7f40c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1002f0c9c:      nop
1002f0ca0:      nop
1002f0ca4:      nop
1002f0ca8:      nop
1002f0cac:      nop
1002f0cb0:      nop
1002f0cb4:      nop
1002f0cb8:      nop
1002f0cbc:      nop
        ...
