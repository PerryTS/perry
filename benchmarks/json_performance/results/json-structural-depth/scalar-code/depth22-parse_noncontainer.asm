/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/depth22-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004d2874 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer>:
1004d2874:      sub sp, sp, #0x90
1004d2878:      stp d9, d8, [sp, #0x20]
1004d287c:      stp x28, x27, [sp, #0x30]
1004d2880:      stp x26, x25, [sp, #0x40]
1004d2884:      stp x24, x23, [sp, #0x50]
1004d2888:      stp x22, x21, [sp, #0x60]
1004d288c:      stp x20, x19, [sp, #0x70]
1004d2890:      stp x29, x30, [sp, #0x80]
1004d2894:      add x29, sp, #0x80
1004d2898:      ldrb    w9, [x0, #0x14]
1004d289c:      orr w8, w9, #0x20
1004d28a0:      cmp w8, #0x7b
1004d28a4:      b.ne    0x1004d28cc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x58>
1004d28a8:      ldp x29, x30, [sp, #0x80]
1004d28ac:      ldp x20, x19, [sp, #0x70]
1004d28b0:      ldp x22, x21, [sp, #0x60]
1004d28b4:      ldp x24, x23, [sp, #0x50]
1004d28b8:      ldp x26, x25, [sp, #0x40]
1004d28bc:      ldp x28, x27, [sp, #0x30]
1004d28c0:      ldp d9, d8, [sp, #0x20]
1004d28c4:      add sp, sp, #0x90
1004d28c8:      b   0x1004d0338 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api10parse_slow>
1004d28cc:      mov x26, #0x0               ; =0
1004d28d0:      sub x28, x1, #0x2
1004d28d4:      mov x12, #-0x20000000000    ; =-2199023255552
1004d28d8:      sub x10, x1, #0x1
1004d28dc:      mov x24, #-0x15             ; =-21
1004d28e0:      mov x8, #0x2600             ; =9728
1004d28e4:      movk    x8, #0x1, lsl #32
1004d28e8:      mov x11, #-0x10000000000    ; =-1099511627776
1004d28ec:      sub x25, x1, #0x2
1004d28f0:      add x21, x12, x1, lsl #40
1004d28f4:      cmp w9, #0x20
1004d28f8:      b.hi    0x1004d2930 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d28fc:      mov w12, w9
1004d2900:      lsr x12, x8, x12
1004d2904:      tbz w12, #0x0, 0x1004d2930 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xbc>
1004d2908:      cmp x10, x26
1004d290c:      b.eq    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2910:      add x9, x0, x26
1004d2914:      ldrb    w9, [x9, #0x15]
1004d2918:      sub x25, x25, #0x1
1004d291c:      add x26, x26, #0x1
1004d2920:      add x21, x21, x11
1004d2924:      sub x24, x24, #0x1
1004d2928:      cmp w9, #0x20
1004d292c:      b.ls    0x1004d28fc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x88>
1004d2930:      add x23, x0, #0x13
1004d2934:      add x11, x0, x26
1004d2938:      mov x8, #0x2600             ; =9728
1004d293c:      movk    x8, #0x1, lsl #32
1004d2940:      mov x13, #-0x10000000000    ; =-1099511627776
1004d2944:      mov x22, x26
1004d2948:      ldrb    w12, [x23, x1]
1004d294c:      cmp w12, #0x20
1004d2950:      b.hi    0x1004d2978 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x104>
1004d2954:      lsr x14, x8, x12
1004d2958:      tbz w14, #0x0, 0x1004d2978 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x104>
1004d295c:      sub x23, x23, #0x1
1004d2960:      add x22, x22, #0x1
1004d2964:      add x21, x21, x13
1004d2968:      sub x25, x25, #0x1
1004d296c:      cmp x1, x22
1004d2970:      b.ne    0x1004d2948 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0xd4>
1004d2974:      b   0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2978:      sub x8, x1, x22
1004d297c:      cmp x8, #0x4
1004d2980:      b.eq    0x1004d29ec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x178>
1004d2984:      cmp x8, #0x5
1004d2988:      b.ne    0x1004d29f4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x180>
1004d298c:      cmp w9, #0x22
1004d2990:      b.eq    0x1004d2af4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x280>
1004d2994:      cmp w9, #0x2d
1004d2998:      b.eq    0x1004d2a10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x19c>
1004d299c:      cmp w9, #0x66
1004d29a0:      b.ne    0x1004d2a04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x190>
1004d29a4:      add x8, x0, x26
1004d29a8:      ldrb    w9, [x8, #0x15]
1004d29ac:      cmp w9, #0x61
1004d29b0:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d29b4:      ldrb    w8, [x8, #0x16]
1004d29b8:      cmp w8, #0x6c
1004d29bc:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d29c0:      add x8, x0, x26
1004d29c4:      ldrb    w9, [x8, #0x17]
1004d29c8:      cmp w9, #0x73
1004d29cc:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d29d0:      ldrb    w8, [x8, #0x18]
1004d29d4:      cmp w8, #0x65
1004d29d8:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d29dc:      mov x8, #0x2                ; =2
1004d29e0:      movk    x8, #0x7ffc, lsl #48
1004d29e4:      orr x19, x8, #0x1
1004d29e8:      b   0x1004d2a3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d29ec:      cmp w9, #0x6d
1004d29f0:      b.gt    0x1004d2b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2e0>
1004d29f4:      cmp w9, #0x22
1004d29f8:      b.eq    0x1004d2af4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x280>
1004d29fc:      cmp w9, #0x2d
1004d2a00:      b.eq    0x1004d2a10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x19c>
1004d2a04:      sub w9, w9, #0x30
1004d2a08:      cmp w9, #0xa
1004d2a0c:      b.hs    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2a10:      mov x19, x0
1004d2a14:      add x0, x11, #0x14
1004d2a18:      mov x20, x1
1004d2a1c:      mov x1, x8
1004d2a20:      bl  0x1004bc148 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar12parse_number>
1004d2a24:      mov x8, x0
1004d2a28:      mov x0, x19
1004d2a2c:      mov x19, x1
1004d2a30:      mov x1, x20
1004d2a34:      cmp x8, #0x1
1004d2a38:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2a3c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2a40:      add x0, x0, #0x9d0
1004d2a44:      ldr x8, [x0]
1004d2a48:      blr x8
1004d2a4c:      ldrb    w9, [x0]
1004d2a50:      strb    wzr, [x0]
1004d2a54:      cbz w9, 0x1004d2a94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x220>
1004d2a58:      mov x8, x0
1004d2a5c:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2a60:      add x0, x0, #0x9e8
1004d2a64:      ldr x9, [x0]
1004d2a68:      blr x9
1004d2a6c:      ldrb    w9, [x0]
1004d2a70:      tst w9, #0x3
1004d2a74:      b.ne    0x1004d2a8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x218>
1004d2a78:      adrp    x9, 0x100fd9000 <_PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD+0xfeec>
1004d2a7c:      add x9, x9, #0x8fc
1004d2a80:      ldapr   w9, [x9]
1004d2a84:      cmp w9, #0x0
1004d2a88:      b.le    0x1004d2c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x39c>
1004d2a8c:      mov w9, #0x1                ; =1
1004d2a90:      strb    w9, [x8]
1004d2a94:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004d2a98:      add x0, x0, #0xb60
1004d2a9c:      ldr x8, [x0]
1004d2aa0:      blr x8
1004d2aa4:      ldrb    w8, [x0, #0x38]
1004d2aa8:      cmp w8, #0x1
1004d2aac:      b.ne    0x1004d2ef0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x67c>
1004d2ab0:      ldr x8, [x0]
1004d2ab4:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1004d2ab8:      cmp x8, x9
1004d2abc:      b.hs    0x1004d2f04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x690>
1004d2ac0:      ldr x8, [x0, #0x20]
1004d2ac4:      cmp x8, #0x1, lsl #12       ; =0x1000
1004d2ac8:      b.hi    0x1004d2f10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x69c>
1004d2acc:      mov x0, x19
1004d2ad0:      ldp x29, x30, [sp, #0x80]
1004d2ad4:      ldp x20, x19, [sp, #0x70]
1004d2ad8:      ldp x22, x21, [sp, #0x60]
1004d2adc:      ldp x24, x23, [sp, #0x50]
1004d2ae0:      ldp x26, x25, [sp, #0x40]
1004d2ae4:      ldp x28, x27, [sp, #0x30]
1004d2ae8:      ldp d9, d8, [sp, #0x20]
1004d2aec:      add sp, sp, #0x90
1004d2af0:      ret
1004d2af4:      cmp x10, x22
1004d2af8:      b.eq    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2afc:      cmp x8, #0x7
1004d2b00:      b.hi    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b04:      cmp w12, #0x22
1004d2b08:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b0c:      sub x19, x8, #0x2
1004d2b10:      cmp x28, x22
1004d2b14:      b.ne    0x1004d2bd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x364>
1004d2b18:      add x8, x0, x26
1004d2b1c:      add x20, x8, #0x15
1004d2b20:      add x8, sp, #0x8
1004d2b24:      str x0, [sp]
1004d2b28:      mov x0, x20
1004d2b2c:      mov x27, x1
1004d2b30:      mov x1, x19
1004d2b34:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1004d2b38:      ldp x0, x8, [sp]
1004d2b3c:      mov x1, x27
1004d2b40:      cbnz    x8, 0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b44:      cmp x28, x22
1004d2b48:      b.ne    0x1004d2c64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3f0>
1004d2b4c:      mov x12, #0x0               ; =0
1004d2b50:      b   0x1004d2ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d2b54:      cmp w9, #0x74
1004d2b58:      b.eq    0x1004d2b9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x328>
1004d2b5c:      cmp w9, #0x6e
1004d2b60:      b.ne    0x1004d2a04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x190>
1004d2b64:      add x8, x0, x26
1004d2b68:      ldrb    w9, [x8, #0x15]
1004d2b6c:      cmp w9, #0x75
1004d2b70:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b74:      ldrb    w8, [x8, #0x16]
1004d2b78:      cmp w8, #0x6c
1004d2b7c:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b80:      add x8, x0, x26
1004d2b84:      ldrb    w8, [x8, #0x17]
1004d2b88:      cmp w8, #0x6c
1004d2b8c:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2b90:      mov x19, #0x2               ; =2
1004d2b94:      movk    x19, #0x7ffc, lsl #48
1004d2b98:      b   0x1004d2a3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d2b9c:      add x8, x0, x26
1004d2ba0:      ldrb    w9, [x8, #0x15]
1004d2ba4:      cmp w9, #0x72
1004d2ba8:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2bac:      ldrb    w8, [x8, #0x16]
1004d2bb0:      cmp w8, #0x75
1004d2bb4:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2bb8:      add x8, x0, x26
1004d2bbc:      ldrb    w8, [x8, #0x17]
1004d2bc0:      cmp w8, #0x65
1004d2bc4:      b.ne    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2bc8:      mov x8, #0x2                ; =2
1004d2bcc:      movk    x8, #0x7ffc, lsl #48
1004d2bd0:      add x19, x8, #0x2
1004d2bd4:      b   0x1004d2a3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d2bd8:      mov x8, #0x0                ; =0
1004d2bdc:      add x9, x0, x8
1004d2be0:      add x9, x9, x26
1004d2be4:      ldrb    w9, [x9, #0x15]
1004d2be8:      cmp w9, #0x20
1004d2bec:      b.lo    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2bf0:      cmp w9, #0x22
1004d2bf4:      b.eq    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2bf8:      cmp w9, #0x5c
1004d2bfc:      b.eq    0x1004d28a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x34>
1004d2c00:      add x8, x8, #0x1
1004d2c04:      cmp x19, x8
1004d2c08:      b.ne    0x1004d2bdc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x368>
1004d2c0c:      b   0x1004d2b18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x2a4>
1004d2c10:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2c14:      add x0, x0, #0xda8
1004d2c18:      ldr x8, [x0]
1004d2c1c:      blr x8
1004d2c20:      ldr x8, [x0]
1004d2c24:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2c28:      add x0, x0, #0x808
1004d2c2c:      ldr x9, [x0]
1004d2c30:      blr x9
1004d2c34:      ldr x9, [x0]
1004d2c38:      cmp x9, x8
1004d2c3c:      b.ls    0x1004d2c5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x3e8>
1004d2c40:      str x8, [x0]
1004d2c44:      adrp    x0, 0x100f02000 <__RNvNCNKNvNtNtCs7wa3bwJW6LN_13perry_runtime13child_process7reactor10CP_PUMPING0s_023___RUST_STD_INTERNAL_VAL+0x8>
1004d2c48:      add x0, x0, #0x778
1004d2c4c:      ldr x8, [x0]
1004d2c50:      blr x8
1004d2c54:      mov w8, #0x1                ; =1
1004d2c58:      strb    w8, [x0]
1004d2c5c:      bl  0x1004344c0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2gc6policy16gc_check_trigger>
1004d2c60:      b   0x1004d2a94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x220>
1004d2c64:      cmp x19, #0x4
1004d2c68:      b.hs    0x1004d2c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x404>
1004d2c6c:      mov x12, #0x0               ; =0
1004d2c70:      mov x8, #0x0                ; =0
1004d2c74:      b   0x1004d2ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x64c>
1004d2c78:      adrp    x10, 0x100c29000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x41a8>
1004d2c7c:      adrp    x9, 0x100bd0000 <__RNvCs8xF4iOrs9m2_4itoa13DECIMAL_PAIRS+0x33d5a>
1004d2c80:      cmp x19, #0x10
1004d2c84:      b.hs    0x1004d2c94 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x420>
1004d2c88:      mov x8, #0x0                ; =0
1004d2c8c:      mov x12, #0x0               ; =0
1004d2c90:      b   0x1004d2e18 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x5a4>
1004d2c94:      mov x12, #0x0               ; =0
1004d2c98:      and x11, x19, #0xc
1004d2c9c:      adrp    x8, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2ca0:      ldr q0, [x8, #0xc0]
1004d2ca4:      adrp    x8, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2ca8:      ldr q1, [x8, #0xd0]
1004d2cac:      and x8, x19, #0xfffffffffffffff0
1004d2cb0:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2cb4:      ldr q2, [x13, #0xe0]
1004d2cb8:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2cbc:      ldr q3, [x13, #0xf0]
1004d2cc0:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2cc4:      ldr q4, [x13, #0x100]
1004d2cc8:      adrp    x13, 0x100c2a000 <__RNvNvNtNtCs7wa3bwJW6LN_13perry_runtime6string6format13fmt_fixed_int5POW10+0x51a8>
1004d2ccc:      ldr q5, [x13, #0x110]
1004d2cd0:      mov w13, #0x10              ; =16
1004d2cd4:      dup.2d  v7, x13
1004d2cd8:      and x13, x25, #0xfffffffffffffff0
1004d2cdc:      add x13, x0, x13
1004d2ce0:      sub x20, x13, x24
1004d2ce4:      movi.2d v6, #0000000000000000
1004d2ce8:      ldr q16, [x10, #0x340]
1004d2cec:      movi.2d v17, #0000000000000000
1004d2cf0:      movi.2d v19, #0000000000000000
1004d2cf4:      ldr q22, [x9, #0x220]
1004d2cf8:      movi.2d v18, #0000000000000000
1004d2cfc:      movi.2d v23, #0000000000000000
1004d2d00:      movi.2d v20, #0000000000000000
1004d2d04:      movi.2d v24, #0000000000000000
1004d2d08:      movi.2d v21, #0000000000000000
1004d2d0c:      add x13, x0, x12
1004d2d10:      add x13, x13, x26
1004d2d14:      ldur    q25, [x13, #0x15]
1004d2d18:      ushll2.8h   v26, v25, #0x0
1004d2d1c:      ushll2.4s   v27, v26, #0x0
1004d2d20:      ushll.2d    v28, v27, #0x0
1004d2d24:      ushll.4s    v26, v26, #0x0
1004d2d28:      ushll2.2d   v29, v26, #0x0
1004d2d2c:      ushll.8h    v25, v25, #0x0
1004d2d30:      ushll2.4s   v30, v25, #0x0
1004d2d34:      ushll2.2d   v31, v30, #0x0
1004d2d38:      ushll2.2d   v27, v27, #0x0
1004d2d3c:      ushll.2d    v26, v26, #0x0
1004d2d40:      ushll.2d    v30, v30, #0x0
1004d2d44:      ushll.4s    v25, v25, #0x0
1004d2d48:      ushll2.2d   v8, v25, #0x0
1004d2d4c:      ushll.2d    v25, v25, #0x0
1004d2d50:      shl.2d  v9, v22, #0x3
1004d2d54:      ushl.2d v25, v25, v9
1004d2d58:      shl.2d  v9, v16, #0x3
1004d2d5c:      ushl.2d v8, v8, v9
1004d2d60:      shl.2d  v9, v5, #0x3
1004d2d64:      ushl.2d v30, v30, v9
1004d2d68:      shl.2d  v9, v3, #0x3
1004d2d6c:      ushl.2d v26, v26, v9
1004d2d70:      shl.2d  v9, v0, #0x3
1004d2d74:      ushl.2d v27, v27, v9
1004d2d78:      shl.2d  v9, v4, #0x3
1004d2d7c:      ushl.2d v31, v31, v9
1004d2d80:      shl.2d  v9, v2, #0x3
1004d2d84:      ushl.2d v29, v29, v9
1004d2d88:      shl.2d  v9, v1, #0x3
1004d2d8c:      ushl.2d v28, v28, v9
1004d2d90:      orr.16b v24, v28, v24
1004d2d94:      orr.16b v20, v29, v20
1004d2d98:      orr.16b v18, v31, v18
1004d2d9c:      orr.16b v21, v27, v21
1004d2da0:      orr.16b v23, v26, v23
1004d2da4:      orr.16b v19, v30, v19
1004d2da8:      orr.16b v6, v8, v6
1004d2dac:      orr.16b v17, v25, v17
1004d2db0:      add x12, x12, #0x10
1004d2db4:      add.2d  v5, v5, v7
1004d2db8:      add.2d  v16, v16, v7
1004d2dbc:      add.2d  v22, v22, v7
1004d2dc0:      add.2d  v4, v4, v7
1004d2dc4:      add.2d  v3, v3, v7
1004d2dc8:      add.2d  v2, v2, v7
1004d2dcc:      add.2d  v1, v1, v7
1004d2dd0:      add.2d  v0, v0, v7
1004d2dd4:      cmp x8, x12
1004d2dd8:      b.ne    0x1004d2d0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x498>
1004d2ddc:      orr.16b v0, v17, v23
1004d2de0:      orr.16b v1, v19, v24
1004d2de4:      orr.16b v0, v0, v1
1004d2de8:      orr.16b v1, v6, v20
1004d2dec:      orr.16b v2, v18, v21
1004d2df0:      orr.16b v1, v1, v2
1004d2df4:      orr.16b v0, v0, v1
1004d2df8:      mov d1, v0[1]
1004d2dfc:      orr.8b  v0, v0, v1
1004d2e00:      fmov    x12, d0
1004d2e04:      cmp x19, x8
1004d2e08:      b.eq    0x1004d2ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d2e0c:      mov x1, x27
1004d2e10:      ldr x0, [sp]
1004d2e14:      cbz x11, 0x1004d2ec0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x64c>
1004d2e18:      mov x11, x8
1004d2e1c:      and x8, x19, #0xfffffffffffffffc
1004d2e20:      and x13, x25, #0xfffffffffffffffc
1004d2e24:      add x13, x0, x13
1004d2e28:      sub x20, x13, x24
1004d2e2c:      fmov    d0, x12
1004d2e30:      movi.2d v1, #0000000000000000
1004d2e34:      dup.2d  v3, x11
1004d2e38:      ldr q2, [x10, #0x340]
1004d2e3c:      orr.16b v2, v3, v2
1004d2e40:      ldr q4, [x9, #0x220]
1004d2e44:      orr.16b v3, v3, v4
1004d2e48:      sub x9, x11, x8
1004d2e4c:      add x10, x0, x11
1004d2e50:      add x10, x10, x26
1004d2e54:      add x10, x10, #0x15
1004d2e58:      movi.2d v4, #0x000000000000ff
1004d2e5c:      mov w11, #0x4               ; =4
1004d2e60:      dup.2d  v5, x11
1004d2e64:      ldr s6, [x10], #0x4
1004d2e68:      ushll.8h    v6, v6, #0x0
1004d2e6c:      ushll.4s    v6, v6, #0x0
1004d2e70:      ushll2.2d   v7, v6, #0x0
1004d2e74:      and.16b v7, v7, v4
1004d2e78:      ushll.2d    v6, v6, #0x0
1004d2e7c:      and.16b v6, v6, v4
1004d2e80:      shl.2d  v16, v2, #0x3
1004d2e84:      shl.2d  v17, v3, #0x3
1004d2e88:      ushl.2d v6, v6, v17
1004d2e8c:      ushl.2d v7, v7, v16
1004d2e90:      orr.16b v1, v7, v1
1004d2e94:      orr.16b v0, v6, v0
1004d2e98:      add.2d  v2, v2, v5
1004d2e9c:      add.2d  v3, v3, v5
1004d2ea0:      adds    x9, x9, #0x4
1004d2ea4:      b.ne    0x1004d2e64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x5f0>
1004d2ea8:      orr.16b v0, v0, v1
1004d2eac:      mov d1, v0[1]
1004d2eb0:      orr.8b  v0, v0, v1
1004d2eb4:      fmov    x12, d0
1004d2eb8:      cmp x19, x8
1004d2ebc:      b.eq    0x1004d2ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x66c>
1004d2ec0:      add x9, x23, x1
1004d2ec4:      lsl x8, x8, #3
1004d2ec8:      ldrb    w10, [x20], #0x1
1004d2ecc:      lsl x10, x10, x8
1004d2ed0:      orr x12, x10, x12
1004d2ed4:      add x8, x8, #0x8
1004d2ed8:      cmp x9, x20
1004d2edc:      b.ne    0x1004d2ec8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x654>
1004d2ee0:      mov x8, #0x7ff9000000000000 ; =9221401712017801216
1004d2ee4:      orr x8, x21, x8
1004d2ee8:      orr x19, x8, x12
1004d2eec:      b   0x1004d2a3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x1c8>
1004d2ef0:      bl  0x100abc4f0 <__RINvMs0_NtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtNtNtBe_11collections4hash3map7HashMapINtNtCsctvjasLqLe9_5alloc3vec3VechEPNtNtCs7wa3bwJW6LN_13perry_runtime6string12StringHeaderEEuE16get_or_init_slowNvNvNtB39_4json15PARSE_KEY_CACHE27___rust_std_internal_init_fnEB39_>
1004d2ef4:      cbnz    x0, 0x1004d2ab0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x23c>
1004d2ef8:      adrp    x0, 0x100e76000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0xfc8>
1004d2efc:      add x0, x0, #0x850
1004d2f00:      bl  0x100aef01c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1004d2f04:      adrp    x0, 0x100e7d000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x7fc8>
1004d2f08:      add x0, x0, #0x3c8
1004d2f0c:      bl  0x100ab0c1c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004d2f10:      bl  0x100ade008 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json12parse_scalar15clear_key_cache>
1004d2f14:      b   0x1004d2acc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json9parse_api18parse_noncontainer+0x258>
