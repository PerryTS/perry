/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010055c93c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object>:
10055c93c:      stp x28, x27, [sp, #-0x60]!
10055c940:      stp x26, x25, [sp, #0x10]
10055c944:      stp x24, x23, [sp, #0x20]
10055c948:      stp x22, x21, [sp, #0x30]
10055c94c:      stp x20, x19, [sp, #0x40]
10055c950:      stp x29, x30, [sp, #0x50]
10055c954:      add x29, sp, #0x50
10055c958:      sub sp, sp, #0x1c0
10055c95c:      mov x19, x1
10055c960:      mov x20, x0
10055c964:      movi.2d v0, #0000000000000000
10055c968:      str d0, [sp, #0x10]
10055c96c:      str wzr, [sp, #0x18]
10055c970:      str d0, [sp, #0x38]
10055c974:      str wzr, [sp, #0x40]
10055c978:      str d0, [sp, #0x60]
10055c97c:      str wzr, [sp, #0x68]
10055c980:      str d0, [sp, #0x88]
10055c984:      str wzr, [sp, #0x90]
10055c988:      str d0, [sp, #0xb0]
10055c98c:      str wzr, [sp, #0xb8]
10055c990:      str d0, [sp, #0xd8]
10055c994:      str wzr, [sp, #0xe0]
10055c998:      str d0, [sp, #0x100]
10055c99c:      str wzr, [sp, #0x108]
10055c9a0:      str d0, [sp, #0x128]
10055c9a4:      str wzr, [sp, #0x130]
10055c9a8:      ldr w21, [x0, #0x4]
10055c9ac:      adrp    x26, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
10055c9b0:      add x26, x26, #0x94
10055c9b4:      ldr w22, [x26]
10055c9b8:      adrp    x25, 0x101118000 <_perry_global_baseline_worker_ts__1>
10055c9bc:      add x25, x25, #0xec8
10055c9c0:      cmp w22, #0x300
10055c9c4:      b.hs    0x10055cfcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
10055c9c8:      ldr x8, [x25]
10055c9cc:      cmn x8, #0x1
10055c9d0:      b.eq    0x10055cfbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
10055c9d4:      mrs x9, TPIDRRO_EL0
10055c9d8:      and x9, x9, #0xfffffffffffffff8
10055c9dc:      ldr x0, [x9, x8, lsl #3]
10055c9e0:      cbz x0, 0x10055cfbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x680>
10055c9e4:      add x8, x0, x22, lsl #3
10055c9e8:      ldr x0, [x8, #0x1e8]
10055c9ec:      cbz x0, 0x10055cfcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x690>
10055c9f0:      ldr x0, [x0]
10055c9f4:      cbz x0, 0x10055cfe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6a4>
10055c9f8:      ldr x8, [x0, #0x5190]
10055c9fc:      ubfx    x9, x21, #15, #15
10055ca00:      ubfx    x10, x21, #5, #10
10055ca04:      and x11, x21, #0x1f
10055ca08:      ldr x8, [x8, x9, lsl #3]
10055ca0c:      ldr x8, [x8, x10, lsl #3]
10055ca10:      lsl x9, x11, #5
10055ca14:      ldr x22, [x8, x9]
10055ca18:      ldr x1, [x22, #0x8]
10055ca1c:      sub x0, x29, #0x90
10055ca20:      bl  0x10055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10055ca24:      ldur    w8, [x29, #-0x90]
10055ca28:      cmn w8, #0x1
10055ca2c:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055ca30:      ldur    x8, [x29, #-0x70]
10055ca34:      ldp q1, q0, [x29, #-0x90]
10055ca38:      stp q1, q0, [sp, #0x10]
10055ca3c:      str x8, [sp, #0x30]
10055ca40:      ldr x1, [x20, #0x10]
10055ca44:      sub x0, x29, #0x90
10055ca48:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10055ca4c:      ldur    w8, [x29, #-0x90]
10055ca50:      cmn w8, #0x1
10055ca54:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055ca58:      ldur    x8, [x29, #-0x70]
10055ca5c:      ldp q1, q0, [x29, #-0x90]
10055ca60:      stp q1, q0, [sp, #0xb0]
10055ca64:      str x8, [sp, #0xd0]
10055ca68:      ldp w10, w8, [sp, #0x10]
10055ca6c:      ldr w9, [sp, #0x18]
10055ca70:      cbz w10, 0x10055ca9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x160>
10055ca74:      cmp w10, #0x1
10055ca78:      b.ne    0x10055cac4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x188>
10055ca7c:      ldr w8, [sp, #0x1c]
10055ca80:      ldp w12, w10, [sp, #0xb0]
10055ca84:      ldr w11, [sp, #0xb8]
10055ca88:      cbz w12, 0x10055cab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
10055ca8c:      cmp w12, #0x1
10055ca90:      b.ne    0x10055cad8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x19c>
10055ca94:      ldr w10, [sp, #0xbc]
10055ca98:      b   0x10055cadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
10055ca9c:      add w10, w8, #0x2
10055caa0:      add w8, w9, #0x2
10055caa4:      mov x9, x10
10055caa8:      ldp w12, w10, [sp, #0xb0]
10055caac:      ldr w11, [sp, #0xb8]
10055cab0:      cbnz    w12, 0x10055ca8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
10055cab4:      add w12, w10, #0x2
10055cab8:      add w10, w11, #0x2
10055cabc:      mov x11, x12
10055cac0:      b   0x10055cadc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1a0>
10055cac4:      mov x9, x8
10055cac8:      ldp w12, w10, [sp, #0xb0]
10055cacc:      ldr w11, [sp, #0xb8]
10055cad0:      cbnz    w12, 0x10055ca8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x150>
10055cad4:      b   0x10055cab4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x178>
10055cad8:      mov x11, x10
10055cadc:      cmn w9, #0x3
10055cae0:      b.hi    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cae4:      add w9, w9, #0x2
10055cae8:      adds    w9, w11, w9
10055caec:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055caf0:      mov x0, #0x0                ; =0
10055caf4:      adds    w21, w9, #0x1
10055caf8:      b.hs    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055cafc:      cmn w8, #0x3
10055cb00:      b.hi    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055cb04:      mov x0, #0x0                ; =0
10055cb08:      add w8, w8, #0x2
10055cb0c:      adds    w24, w10, w8
10055cb10:      b.hs    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055cb14:      cmn w24, #0x1
10055cb18:      b.eq    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055cb1c:      add w27, w24, #0x1
10055cb20:      cmp x19, #0x1
10055cb24:      b.ne    0x10055cb64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x228>
10055cb28:      ldr x8, [x25]
10055cb2c:      cmn x8, #0x1
10055cb30:      b.eq    0x10055cbdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
10055cb34:      mrs x9, TPIDRRO_EL0
10055cb38:      and x9, x9, #0xfffffffffffffff8
10055cb3c:      ldr x8, [x9, x8, lsl #3]
10055cb40:      cbz x8, 0x10055cbdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2a0>
10055cb44:      ldr x8, [x8, #0x19e8]
10055cb48:      cbz x8, 0x10055cfe8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6ac>
10055cb4c:      ldr x9, [x8]
10055cb50:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10055cb54:      cmp x9, x10
10055cb58:      b.hs    0x10055d378 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa3c>
10055cb5c:      ldr x22, [x8, #0x18]
10055cb60:      b   0x10055cc08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
10055cb64:      ldr x1, [x22, #0x10]
10055cb68:      sub x0, x29, #0x90
10055cb6c:      bl  0x10055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10055cb70:      ldur    w8, [x29, #-0x90]
10055cb74:      cmn w8, #0x1
10055cb78:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cb7c:      ldur    x8, [x29, #-0x70]
10055cb80:      ldp q1, q0, [x29, #-0x90]
10055cb84:      stur    q1, [sp, #0x38]
10055cb88:      stur    q0, [sp, #0x48]
10055cb8c:      str x8, [sp, #0x58]
10055cb90:      ldr x1, [x20, #0x18]
10055cb94:      sub x0, x29, #0x90
10055cb98:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10055cb9c:      ldur    w8, [x29, #-0x90]
10055cba0:      cmn w8, #0x1
10055cba4:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cba8:      add x23, sp, #0xb0
10055cbac:      ldur    x8, [x29, #-0x70]
10055cbb0:      ldp q1, q0, [x29, #-0x90]
10055cbb4:      stur    q1, [x23, #0x28]
10055cbb8:      stur    q0, [x23, #0x38]
10055cbbc:      str x8, [sp, #0xf8]
10055cbc0:      ldp w10, w8, [sp, #0x38]
10055cbc4:      ldr w9, [sp, #0x40]
10055cbc8:      cbz w10, 0x10055cffc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6c0>
10055cbcc:      cmp w10, #0x1
10055cbd0:      b.ne    0x10055d00c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d0>
10055cbd4:      ldr w8, [sp, #0x44]
10055cbd8:      b   0x10055d010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
10055cbdc:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10055cbe0:      add x0, x0, #0x550
10055cbe4:      ldr x8, [x0]
10055cbe8:      blr x8
10055cbec:      ldrb    w8, [x0, #0x20]
10055cbf0:      cbnz    w8, 0x10055d32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9f0>
10055cbf4:      ldr x8, [x0]
10055cbf8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10055cbfc:      cmp x8, x9
10055cc00:      b.hs    0x10055d35c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa20>
10055cc04:      ldr x22, [x0, #0x18]
10055cc08:      stur    x22, [x29, #-0x68]
10055cc0c:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
10055cc10:      stp x20, x8, [x29, #-0x88]
10055cc14:      mov w8, #0x1                ; =1
10055cc18:      stur    x8, [x29, #-0x90]
10055cc1c:      sub x0, x29, #0x90
10055cc20:      bl  0x1005286e0 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
10055cc24:      mov x24, x0
10055cc28:      stur    x0, [x29, #-0xc0]
10055cc2c:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10055cc30:      add x0, x0, #0x4c0
10055cc34:      ldr x8, [x0]
10055cc38:      blr x8
10055cc3c:      strb    wzr, [x0]
10055cc40:      mov x0, x20
10055cc44:      bl  0x1002f08c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
10055cc48:      tbz w0, #0x0, 0x10055ccd8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x39c>
10055cc4c:      mov x0, x21
10055cc50:      bl  0x1005356ec <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
10055cc54:      mov x20, x0
10055cc58:      mov x23, x1
10055cc5c:      ldr x8, [x25]
10055cc60:      cmn x8, #0x1
10055cc64:      b.eq    0x10055cd1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
10055cc68:      mrs x9, TPIDRRO_EL0
10055cc6c:      and x9, x9, #0xfffffffffffffff8
10055cc70:      ldr x8, [x9, x8, lsl #3]
10055cc74:      cbz x8, 0x10055cd1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3e0>
10055cc78:      ldr x8, [x8, #0x19e8]
10055cc7c:      cbz x8, 0x10055d0f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7b8>
10055cc80:      ldr x9, [x8]
10055cc84:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10055cc88:      cmp x9, x10
10055cc8c:      b.hs    0x10055d414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xad8>
10055cc90:      add x10, x9, #0x1
10055cc94:      str x10, [x8]
10055cc98:      ldr x10, [x8, #0x18]
10055cc9c:      cmp x24, x10
10055cca0:      b.hs    0x10055d328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
10055cca4:      ldr x10, [x8, #0x10]
10055cca8:      mov w11, #0x18              ; =24
10055ccac:      madd    x10, x24, x11, x10
10055ccb0:      ldr x11, [x10]
10055ccb4:      cmp x11, #0x1
10055ccb8:      b.ne    0x10055d420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xae4>
10055ccbc:      ldr x24, [x10, #0x8]
10055ccc0:      str x9, [x8]
10055ccc4:      ldr w28, [x24, #0x4]
10055ccc8:      ldr w26, [x26]
10055cccc:      cmp w26, #0x300
10055ccd0:      b.lo    0x10055cd88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
10055ccd4:      b   0x10055d180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10055ccd8:      ldr x8, [x25]
10055ccdc:      cmn x8, #0x1
10055cce0:      b.eq    0x10055cf54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
10055cce4:      mrs x9, TPIDRRO_EL0
10055cce8:      and x9, x9, #0xfffffffffffffff8
10055ccec:      ldr x8, [x9, x8, lsl #3]
10055ccf0:      cbz x8, 0x10055cf54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x618>
10055ccf4:      ldr x8, [x8, #0x19e8]
10055ccf8:      cbz x8, 0x10055d11c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7e0>
10055ccfc:      ldr x9, [x8]
10055cd00:      cbnz    x9, 0x10055d384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
10055cd04:      ldr x9, [x8, #0x18]
10055cd08:      cmp x22, x9
10055cd0c:      b.hi    0x10055cd14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x3d8>
10055cd10:      str x22, [x8, #0x18]
10055cd14:      str xzr, [x8]
10055cd18:      b   0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cd1c:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10055cd20:      add x0, x0, #0x550
10055cd24:      ldr x8, [x0]
10055cd28:      blr x8
10055cd2c:      ldrb    w8, [x0, #0x20]
10055cd30:      cbnz    w8, 0x10055d390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa54>
10055cd34:      ldr x8, [x0]
10055cd38:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10055cd3c:      cmp x8, x9
10055cd40:      b.hs    0x10055d3c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa8c>
10055cd44:      add x9, x8, #0x1
10055cd48:      str x9, [x0]
10055cd4c:      ldr x9, [x0, #0x18]
10055cd50:      cmp x24, x9
10055cd54:      b.hs    0x10055d328 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ec>
10055cd58:      ldr x9, [x0, #0x10]
10055cd5c:      mov w10, #0x18              ; =24
10055cd60:      madd    x9, x24, x10, x9
10055cd64:      ldr x10, [x9]
10055cd68:      cmp x10, #0x1
10055cd6c:      b.ne    0x10055d368 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa2c>
10055cd70:      ldr x24, [x9, #0x8]
10055cd74:      str x8, [x0]
10055cd78:      ldr w28, [x24, #0x4]
10055cd7c:      ldr w26, [x26]
10055cd80:      cmp w26, #0x300
10055cd84:      b.hs    0x10055d180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10055cd88:      ldr x8, [x25]
10055cd8c:      cmn x8, #0x1
10055cd90:      b.eq    0x10055d170 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
10055cd94:      mrs x9, TPIDRRO_EL0
10055cd98:      and x9, x9, #0xfffffffffffffff8
10055cd9c:      ldr x0, [x9, x8, lsl #3]
10055cda0:      cbz x0, 0x10055d170 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x834>
10055cda4:      add x8, x0, x26, lsl #3
10055cda8:      ldr x0, [x8, #0x1e8]
10055cdac:      cbz x0, 0x10055d180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10055cdb0:      str x22, [sp, #0x8]
10055cdb4:      ldr x0, [x0]
10055cdb8:      cbz x0, 0x10055d198 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x85c>
10055cdbc:      ldr x8, [x0, #0x5190]
10055cdc0:      ubfx    x9, x28, #15, #15
10055cdc4:      ubfx    x10, x28, #5, #10
10055cdc8:      and x11, x28, #0x1f
10055cdcc:      ldr x8, [x8, x9, lsl #3]
10055cdd0:      ldr x8, [x8, x10, lsl #3]
10055cdd4:      lsl x9, x11, #5
10055cdd8:      ldr x26, [x8, x9]
10055cddc:      stp w27, w21, [x20]
10055cde0:      stp wzr, wzr, [x20, #0xc]
10055cde4:      str w21, [x20, #0x8]
10055cde8:      mov w8, #0x7b               ; =123
10055cdec:      mov x2, x23
10055cdf0:      strb    w8, [x2], #0x1
10055cdf4:      ldr x1, [x26, #0x8]
10055cdf8:      add x21, sp, #0x10
10055cdfc:      add x0, sp, #0x10
10055ce00:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055ce04:      add x8, x23, x0
10055ce08:      mov w27, #0x3a              ; =58
10055ce0c:      strb    w27, [x8, #0x1]
10055ce10:      add x22, x0, #0x2
10055ce14:      ldr x1, [x24, #0x10]
10055ce18:      add x28, sp, #0xb0
10055ce1c:      add x0, sp, #0xb0
10055ce20:      add x2, x23, x22
10055ce24:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055ce28:      add x8, x0, x22
10055ce2c:      cmp x19, #0x1
10055ce30:      b.eq    0x10055cf04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10055ce34:      mov w9, #0x2c               ; =44
10055ce38:      strb    w9, [x23, x8]
10055ce3c:      add x22, x8, #0x1
10055ce40:      ldr x1, [x26, #0x10]
10055ce44:      add x0, x21, #0x28
10055ce48:      add x2, x23, x22
10055ce4c:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055ce50:      add x8, x0, x22
10055ce54:      strb    w27, [x23, x8]
10055ce58:      add x21, x8, #0x1
10055ce5c:      ldr x1, [x24, #0x18]
10055ce60:      add x0, x28, #0x28
10055ce64:      add x2, x23, x21
10055ce68:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055ce6c:      add x8, x0, x21
10055ce70:      cmp x19, #0x2
10055ce74:      b.eq    0x10055cf04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10055ce78:      mov w9, #0x2c               ; =44
10055ce7c:      strb    w9, [x23, x8]
10055ce80:      add x22, x8, #0x1
10055ce84:      add x21, sp, #0x10
10055ce88:      ldr x1, [x26, #0x18]
10055ce8c:      add x0, x21, #0x50
10055ce90:      add x2, x23, x22
10055ce94:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055ce98:      add x8, x0, x22
10055ce9c:      mov w28, #0x3a              ; =58
10055cea0:      strb    w28, [x23, x8]
10055cea4:      add x22, x8, #0x1
10055cea8:      add x27, sp, #0xb0
10055ceac:      ldr x1, [x24, #0x20]
10055ceb0:      add x0, x27, #0x50
10055ceb4:      add x2, x23, x22
10055ceb8:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055cebc:      add x8, x0, x22
10055cec0:      cmp x19, #0x3
10055cec4:      b.eq    0x10055cf04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x5c8>
10055cec8:      mov w9, #0x2c               ; =44
10055cecc:      strb    w9, [x23, x8]
10055ced0:      add x19, x8, #0x1
10055ced4:      ldr x1, [x26, #0x20]
10055ced8:      add x0, x21, #0x78
10055cedc:      add x2, x23, x19
10055cee0:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055cee4:      add x8, x0, x19
10055cee8:      strb    w28, [x23, x8]
10055ceec:      add x19, x8, #0x1
10055cef0:      ldr x1, [x24, #0x28]
10055cef4:      add x0, x27, #0x78
10055cef8:      add x2, x23, x19
10055cefc:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
10055cf00:      add x8, x0, x19
10055cf04:      ldr x10, [sp, #0x8]
10055cf08:      mov w9, #0x7d               ; =125
10055cf0c:      strb    w9, [x23, x8]
10055cf10:      ldr x8, [x25]
10055cf14:      cmn x8, #0x1
10055cf18:      b.eq    0x10055cf88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
10055cf1c:      mrs x9, TPIDRRO_EL0
10055cf20:      and x9, x9, #0xfffffffffffffff8
10055cf24:      ldr x8, [x9, x8, lsl #3]
10055cf28:      cbz x8, 0x10055cf88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x64c>
10055cf2c:      ldr x8, [x8, #0x19e8]
10055cf30:      cbz x8, 0x10055d150 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x814>
10055cf34:      ldr x9, [x8]
10055cf38:      cbnz    x9, 0x10055d384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa48>
10055cf3c:      ldr x9, [x8, #0x18]
10055cf40:      cmp x10, x9
10055cf44:      b.hi    0x10055cf4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x610>
10055cf48:      str x10, [x8, #0x18]
10055cf4c:      str xzr, [x8]
10055cf50:      b   0x10055d160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10055cf54:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10055cf58:      add x0, x0, #0x550
10055cf5c:      ldr x8, [x0]
10055cf60:      blr x8
10055cf64:      ldrb    w8, [x0, #0x20]
10055cf68:      cbnz    w8, 0x10055d3d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xa98>
10055cf6c:      ldr x8, [x0]
10055cf70:      cbnz    x8, 0x10055d454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10055cf74:      ldr x8, [x0, #0x18]
10055cf78:      cmp x22, x8
10055cf7c:      b.hi    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cf80:      str x22, [x0, #0x18]
10055cf84:      b   0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055cf88:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10055cf8c:      add x0, x0, #0x550
10055cf90:      ldr x8, [x0]
10055cf94:      blr x8
10055cf98:      ldrb    w8, [x0, #0x20]
10055cf9c:      cbnz    w8, 0x10055d400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xac4>
10055cfa0:      ldr x8, [x0]
10055cfa4:      cbnz    x8, 0x10055d454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10055cfa8:      ldr x8, [x0, #0x18]
10055cfac:      cmp x10, x8
10055cfb0:      b.hi    0x10055d160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10055cfb4:      str x10, [x0, #0x18]
10055cfb8:      b   0x10055d160 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x824>
10055cfbc:      bl  0x100c8b6c8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10055cfc0:      add x8, x0, x22, lsl #3
10055cfc4:      ldr x0, [x8, #0x1e8]
10055cfc8:      cbnz    x0, 0x10055c9f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb4>
10055cfcc:      adrp    x0, 0x1010a3000 <_anon.7adc4553ee057240d1951d2053fb5027.1693+0x1c8>
10055cfd0:      add x0, x0, #0x218
10055cfd4:      bl  0x100c8ae04 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10055cfd8:      ldr x0, [x0]
10055cfdc:      cbnz    x0, 0x10055c9f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
10055cfe0:      bl  0x100cb2b78 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
10055cfe4:      b   0x10055c9f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xbc>
10055cfe8:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055cfec:      add x0, x0, #0x838
10055cff0:      bl  0x1001353ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
10055cff4:      mov x22, x0
10055cff8:      b   0x10055cc08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2cc>
10055cffc:      add w10, w8, #0x2
10055d000:      add w8, w9, #0x2
10055d004:      mov x9, x10
10055d008:      b   0x10055d010 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6d4>
10055d00c:      mov x9, x8
10055d010:      ldp w12, w10, [sp, #0xd8]
10055d014:      ldr w11, [sp, #0xe0]
10055d018:      cbz w12, 0x10055d02c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x6f0>
10055d01c:      cmp w12, #0x1
10055d020:      b.ne    0x10055d03c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x700>
10055d024:      ldr w10, [sp, #0xe4]
10055d028:      b   0x10055d040 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
10055d02c:      add w12, w10, #0x2
10055d030:      add w10, w11, #0x2
10055d034:      mov x11, x12
10055d038:      b   0x10055d040 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x704>
10055d03c:      mov x11, x10
10055d040:      adds    w9, w9, w21
10055d044:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d048:      adds    w9, w11, w9
10055d04c:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d050:      cmn w9, #0x3
10055d054:      b.hi    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d058:      add w8, w8, w27
10055d05c:      cmp w8, w24
10055d060:      b.ls    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d064:      mov x0, #0x0                ; =0
10055d068:      adds    w8, w10, w8
10055d06c:      b.hs    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d070:      cmn w8, #0x3
10055d074:      b.hi    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d078:      add w21, w9, #0x2
10055d07c:      add w27, w8, #0x2
10055d080:      cmp x19, #0x2
10055d084:      b.eq    0x10055cb28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10055d088:      ldr x1, [x22, #0x18]
10055d08c:      sub x0, x29, #0x90
10055d090:      bl  0x10055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10055d094:      ldur    w8, [x29, #-0x90]
10055d098:      cmn w8, #0x1
10055d09c:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d0a0:      ldur    x8, [x29, #-0x70]
10055d0a4:      ldp q1, q0, [x29, #-0x90]
10055d0a8:      stp q1, q0, [sp, #0x60]
10055d0ac:      str x8, [sp, #0x80]
10055d0b0:      ldr x1, [x20, #0x20]
10055d0b4:      sub x0, x29, #0x90
10055d0b8:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10055d0bc:      ldur    w8, [x29, #-0x90]
10055d0c0:      cmn w8, #0x1
10055d0c4:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d0c8:      ldur    x8, [x29, #-0x70]
10055d0cc:      ldp q1, q0, [x29, #-0x90]
10055d0d0:      stp q1, q0, [sp, #0x100]
10055d0d4:      str x8, [sp, #0x120]
10055d0d8:      ldp w10, w8, [sp, #0x60]
10055d0dc:      ldr w9, [sp, #0x68]
10055d0e0:      cbz w10, 0x10055d1a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x864>
10055d0e4:      cmp w10, #0x1
10055d0e8:      b.ne    0x10055d1b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x874>
10055d0ec:      ldr w8, [sp, #0x6c]
10055d0f0:      b   0x10055d1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
10055d0f4:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d0f8:      add x0, x0, #0x838
10055d0fc:      sub x1, x29, #0xc0
10055d100:      bl  0x1001351d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10055d104:      mov x24, x0
10055d108:      ldr w28, [x0, #0x4]
10055d10c:      ldr w26, [x26]
10055d110:      cmp w26, #0x300
10055d114:      b.lo    0x10055cd88 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x44c>
10055d118:      b   0x10055d180 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x844>
10055d11c:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d120:      add x0, x0, #0x838
10055d124:      sub x1, x29, #0x68
10055d128:      bl  0x100135788 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
10055d12c:      mov x0, #0x0                ; =0
10055d130:      add sp, sp, #0x1c0
10055d134:      ldp x29, x30, [sp, #0x50]
10055d138:      ldp x20, x19, [sp, #0x40]
10055d13c:      ldp x22, x21, [sp, #0x30]
10055d140:      ldp x24, x23, [sp, #0x20]
10055d144:      ldp x26, x25, [sp, #0x10]
10055d148:      ldp x28, x27, [sp], #0x60
10055d14c:      ret
10055d150:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d154:      add x0, x0, #0x838
10055d158:      sub x1, x29, #0x68
10055d15c:      bl  0x100135788 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
10055d160:      mov x1, #0x7fff000000000000 ; =9223090561878065152
10055d164:      bfxil   x1, x20, #0, #48
10055d168:      mov w0, #0x1                ; =1
10055d16c:      b   0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d170:      bl  0x100c8b6c8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10055d174:      add x8, x0, x26, lsl #3
10055d178:      ldr x0, [x8, #0x1e8]
10055d17c:      cbnz    x0, 0x10055cdb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x474>
10055d180:      adrp    x0, 0x1010a3000 <_anon.7adc4553ee057240d1951d2053fb5027.1693+0x1c8>
10055d184:      add x0, x0, #0x218
10055d188:      bl  0x100c8ae04 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
10055d18c:      str x22, [sp, #0x8]
10055d190:      ldr x0, [x0]
10055d194:      cbnz    x0, 0x10055cdbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
10055d198:      bl  0x100cb2b78 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
10055d19c:      b   0x10055cdbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x480>
10055d1a0:      add w10, w8, #0x2
10055d1a4:      add w8, w9, #0x2
10055d1a8:      mov x9, x10
10055d1ac:      b   0x10055d1b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x878>
10055d1b0:      mov x9, x8
10055d1b4:      ldr w12, [sp, #0x100]
10055d1b8:      ldr w10, [sp, #0x104]
10055d1bc:      ldr w11, [sp, #0x108]
10055d1c0:      cbz w12, 0x10055d1d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x898>
10055d1c4:      cmp w12, #0x1
10055d1c8:      b.ne    0x10055d1e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8a8>
10055d1cc:      ldr w10, [sp, #0x10c]
10055d1d0:      b   0x10055d1e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
10055d1d4:      add w12, w10, #0x2
10055d1d8:      add w10, w11, #0x2
10055d1dc:      mov x11, x12
10055d1e0:      b   0x10055d1e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x8ac>
10055d1e4:      mov x11, x10
10055d1e8:      adds    w9, w9, w21
10055d1ec:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d1f0:      adds    w9, w11, w9
10055d1f4:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d1f8:      cmn w9, #0x3
10055d1fc:      b.hi    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d200:      adds    w8, w8, w27
10055d204:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d208:      mov x0, #0x0                ; =0
10055d20c:      adds    w8, w10, w8
10055d210:      b.hs    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d214:      cmn w8, #0x3
10055d218:      b.hi    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d21c:      add w21, w9, #0x2
10055d220:      add w27, w8, #0x2
10055d224:      cmp x19, #0x3
10055d228:      b.eq    0x10055cb28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10055d22c:      ldr x1, [x22, #0x20]
10055d230:      sub x0, x29, #0x90
10055d234:      bl  0x10055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10055d238:      ldur    w8, [x29, #-0x90]
10055d23c:      cmn w8, #0x1
10055d240:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d244:      ldur    x8, [x29, #-0x70]
10055d248:      ldp q1, q0, [x29, #-0x90]
10055d24c:      stur    q1, [sp, #0x88]
10055d250:      stur    q0, [sp, #0x98]
10055d254:      str x8, [sp, #0xa8]
10055d258:      ldr x1, [x20, #0x28]
10055d25c:      sub x0, x29, #0x90
10055d260:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
10055d264:      ldur    w8, [x29, #-0x90]
10055d268:      cmn w8, #0x1
10055d26c:      b.eq    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d270:      ldur    x8, [x29, #-0x70]
10055d274:      ldp q1, q0, [x29, #-0x90]
10055d278:      stur    q1, [x23, #0x78]
10055d27c:      stur    q0, [x23, #0x88]
10055d280:      str x8, [sp, #0x148]
10055d284:      ldp w10, w8, [sp, #0x88]
10055d288:      ldr w9, [sp, #0x90]
10055d28c:      cbz w10, 0x10055d2a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x964>
10055d290:      cmp w10, #0x1
10055d294:      b.ne    0x10055d2b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x974>
10055d298:      ldr w8, [sp, #0x94]
10055d29c:      b   0x10055d2b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
10055d2a0:      add w10, w8, #0x2
10055d2a4:      add w8, w9, #0x2
10055d2a8:      mov x9, x10
10055d2ac:      b   0x10055d2b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x978>
10055d2b0:      mov x9, x8
10055d2b4:      ldr w12, [sp, #0x128]
10055d2b8:      ldr w10, [sp, #0x12c]
10055d2bc:      ldr w11, [sp, #0x130]
10055d2c0:      cbz w12, 0x10055d2d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x998>
10055d2c4:      cmp w12, #0x1
10055d2c8:      b.ne    0x10055d2e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9a8>
10055d2cc:      ldr w10, [sp, #0x134]
10055d2d0:      b   0x10055d2e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
10055d2d4:      add w12, w10, #0x2
10055d2d8:      add w10, w11, #0x2
10055d2dc:      mov x11, x12
10055d2e0:      b   0x10055d2e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x9ac>
10055d2e4:      mov x11, x10
10055d2e8:      adds    w9, w9, w21
10055d2ec:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d2f0:      adds    w9, w11, w9
10055d2f4:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d2f8:      cmn w9, #0x3
10055d2fc:      b.hi    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d300:      adds    w8, w8, w27
10055d304:      b.hs    0x10055d12c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f0>
10055d308:      mov x0, #0x0                ; =0
10055d30c:      adds    w8, w10, w8
10055d310:      b.hs    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d314:      cmn w8, #0x3
10055d318:      b.hi    0x10055d130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x7f4>
10055d31c:      add w21, w9, #0x2
10055d320:      add w27, w8, #0x2
10055d324:      b   0x10055cb28 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x1ec>
10055d328:      bl  0x100ca9614 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
10055d32c:      cmp w8, #0x1
10055d330:      b.ne    0x10055d408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10055d334:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
10055d338:      add x1, x1, #0x9c4
10055d33c:      mov x22, x0
10055d340:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10055d344:      mov x0, x22
10055d348:      strb    wzr, [x22, #0x20]
10055d34c:      ldr x8, [x22]
10055d350:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10055d354:      cmp x8, x9
10055d358:      b.lo    0x10055cc04 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x2c8>
10055d35c:      adrp    x0, 0x101088000 <_anon.68a532d94142320e15103d7866c451bd.21>
10055d360:      add x0, x0, #0x468
10055d364:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10055d368:      adrp    x0, 0x100dab000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x22a0>
10055d36c:      add x0, x0, #0xbf0
10055d370:      mov w1, #0xb                ; =11
10055d374:      bl  0x100ca95dc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10055d378:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d37c:      add x0, x0, #0x918
10055d380:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10055d384:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d388:      add x0, x0, #0x9f0
10055d38c:      bl  0x100c7f2ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10055d390:      cmp w8, #0x2
10055d394:      b.eq    0x10055d408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10055d398:      mov x28, x25
10055d39c:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
10055d3a0:      add x1, x1, #0x9c4
10055d3a4:      mov x25, x0
10055d3a8:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10055d3ac:      mov x0, x25
10055d3b0:      strb    wzr, [x25, #0x20]
10055d3b4:      mov x25, x28
10055d3b8:      ldr x8, [x0]
10055d3bc:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10055d3c0:      cmp x8, x9
10055d3c4:      b.lo    0x10055cd44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x408>
10055d3c8:      adrp    x0, 0x101087000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10055d3cc:      add x0, x0, #0xf70
10055d3d0:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10055d3d4:      cmp w8, #0x2
10055d3d8:      b.eq    0x10055d408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xacc>
10055d3dc:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
10055d3e0:      add x1, x1, #0x9c4
10055d3e4:      mov x19, x0
10055d3e8:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10055d3ec:      mov x0, x19
10055d3f0:      strb    wzr, [x19, #0x20]
10055d3f4:      ldr x8, [x19]
10055d3f8:      cbz x8, 0x10055cf74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x638>
10055d3fc:      b   0x10055d454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xb18>
10055d400:      cmp w8, #0x2
10055d404:      b.ne    0x10055d430 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0xaf4>
10055d408:      adrp    x0, 0x101087000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10055d40c:      add x0, x0, #0xed8
10055d410:      bl  0x100cc53dc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
10055d414:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
10055d418:      add x0, x0, #0x8a0
10055d41c:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10055d420:      adrp    x0, 0x100dcd000 <_anon.e80c450fb99efd852d6d235001180335.1329+0x8>
10055d424:      add x0, x0, #0xeac
10055d428:      mov w1, #0xb                ; =11
10055d42c:      bl  0x100ca95dc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
10055d430:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
10055d434:      add x1, x1, #0x9c4
10055d438:      mov x19, x0
10055d43c:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10055d440:      mov x0, x19
10055d444:      strb    wzr, [x19, #0x20]
10055d448:      ldr x10, [sp, #0x8]
10055d44c:      ldr x8, [x19]
10055d450:      cbz x8, 0x10055cfa8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat11emit_object+0x66c>
10055d454:      adrp    x0, 0x10108d000 <_anon.68a532d94142320e15103d7866c451bd.1142>
10055d458:      add x0, x0, #0x270
10055d45c:      bl  0x100c7f2ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
