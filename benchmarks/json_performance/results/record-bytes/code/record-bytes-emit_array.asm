/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008efca0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array>:
1008efca0:      stp x28, x27, [sp, #-0x50]!
1008efca4:      stp x24, x23, [sp, #0x10]
1008efca8:      stp x22, x21, [sp, #0x20]
1008efcac:      stp x20, x19, [sp, #0x30]
1008efcb0:      stp x29, x30, [sp, #0x40]
1008efcb4:      add x29, sp, #0x40
1008efcb8:      sub sp, sp, #0x1, lsl #12   ; =0x1000
1008efcbc:      ldr xzr, [sp]
1008efcc0:      sub sp, sp, #0x1, lsl #12   ; =0x1000
1008efcc4:      ldr xzr, [sp]
1008efcc8:      sub sp, sp, #0x3a0
1008efccc:      mov x19, x2
1008efcd0:      mov x21, x1
1008efcd4:      ldr x20, [x2, #0x10]
1008efcd8:      movi.2d v0, #0000000000000000
1008efcdc:      stur    q0, [sp, #0x88]
1008efce0:      stur    q0, [sp, #0x78]
1008efce4:      stur    q0, [sp, #0x68]
1008efce8:      stur    q0, [sp, #0x58]
1008efcec:      stur    q0, [sp, #0x48]
1008efcf0:      stur    q0, [sp, #0x38]
1008efcf4:      stur    q0, [sp, #0x28]
1008efcf8:      stur    q0, [sp, #0x18]
1008efcfc:      str xzr, [sp, #0x2398]
1008efd00:      mov w8, #0x1                ; =1
1008efd04:      stp xzr, x8, [sp]
1008efd08:      str xzr, [sp, #0x10]
1008efd0c:      ldr x8, [x2]
1008efd10:      cmp x8, x20
1008efd14:      b.eq    0x1008efe6c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x1cc>
1008efd18:      ldr x8, [x19, #0x8]
1008efd1c:      mov w9, #0x5b               ; =91
1008efd20:      strb    w9, [x8, x20]
1008efd24:      add x22, x20, #0x1
1008efd28:      str x22, [x19, #0x10]
1008efd2c:      ldr w23, [x0]
1008efd30:      cbz w23, 0x1008efd58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xb8>
1008efd34:      ldr x1, [x21]
1008efd38:      mov x0, sp
1008efd3c:      mov w2, #0x1                ; =1
1008efd40:      mov x3, x19
1008efd44:      bl  0x1008b4b80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1008efd48:      cbz w0, 0x1008efe1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x17c>
1008efd4c:      cmp w23, #0x1
1008efd50:      b.ne    0x1008efdb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x110>
1008efd54:      ldr x22, [x19, #0x10]
1008efd58:      ldr x8, [x19]
1008efd5c:      cmp x8, x22
1008efd60:      b.eq    0x1008efe90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x1f0>
1008efd64:      ldr x8, [x19, #0x8]
1008efd68:      mov w9, #0x5d               ; =93
1008efd6c:      strb    w9, [x8, x22]
1008efd70:      add x20, x22, #0x1
1008efd74:      mov w21, #0x1               ; =1
1008efd78:      str x20, [x19, #0x10]
1008efd7c:      ldr x8, [sp]
1008efd80:      cbz x8, 0x1008efd8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
1008efd84:      ldr x0, [sp, #0x8]
1008efd88:      bl  0x100ce70c0 <_mi_free>
1008efd8c:      mov x0, x21
1008efd90:      add sp, sp, #0x2, lsl #12   ; =0x2000
1008efd94:      add sp, sp, #0x3a0
1008efd98:      ldp x29, x30, [sp, #0x40]
1008efd9c:      ldp x20, x19, [sp, #0x30]
1008efda0:      ldp x22, x21, [sp, #0x20]
1008efda4:      ldp x24, x23, [sp, #0x10]
1008efda8:      ldp x28, x27, [sp], #0x50
1008efdac:      ret
1008efdb0:      add x22, x21, #0x8
1008efdb4:      sub x23, x23, #0x1
1008efdb8:      mov w24, #0x2c              ; =44
1008efdbc:      ldr x21, [x19, #0x10]
1008efdc0:      ldr x8, [x19]
1008efdc4:      cmp x8, x21
1008efdc8:      b.eq    0x1008efe00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x160>
1008efdcc:      ldr x8, [x19, #0x8]
1008efdd0:      strb    w24, [x8, x21]
1008efdd4:      add x8, x21, #0x1
1008efdd8:      str x8, [x19, #0x10]
1008efddc:      ldr x1, [x22], #0x8
1008efde0:      mov x0, sp
1008efde4:      mov w2, #0x1                ; =1
1008efde8:      mov x3, x19
1008efdec:      bl  0x1008b4b80 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1008efdf0:      tbz w0, #0x0, 0x1008efe1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x17c>
1008efdf4:      subs    x23, x23, #0x1
1008efdf8:      b.ne    0x1008efdbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x11c>
1008efdfc:      b   0x1008efd54 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xb4>
1008efe00:      mov x0, x19
1008efe04:      mov x1, x21
1008efe08:      mov w2, #0x1                ; =1
1008efe0c:      mov w3, #0x1                ; =1
1008efe10:      mov w4, #0x1                ; =1
1008efe14:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008efe18:      b   0x1008efdcc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x12c>
1008efe1c:      ldr x8, [x19, #0x10]
1008efe20:      cmp x20, x8
1008efe24:      b.ls    0x1008efe38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x198>
1008efe28:      mov w21, #0x0               ; =0
1008efe2c:      ldr x8, [sp]
1008efe30:      cbnz    x8, 0x1008efd84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xe4>
1008efe34:      b   0x1008efd8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
1008efe38:      mov w21, #0x0               ; =0
1008efe3c:      cbz x20, 0x1008efd78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xd8>
1008efe40:      cmp x20, x8
1008efe44:      b.hs    0x1008efd78 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xd8>
1008efe48:      ldr x8, [x19, #0x8]
1008efe4c:      ldrsb   w8, [x8, x20]
1008efe50:      cmn w8, #0x41
1008efe54:      b.le    0x1008efeac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x20c>
1008efe58:      mov w21, #0x0               ; =0
1008efe5c:      str x20, [x19, #0x10]
1008efe60:      ldr x8, [sp]
1008efe64:      cbnz    x8, 0x1008efd84 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xe4>
1008efe68:      b   0x1008efd8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
1008efe6c:      mov x22, x0
1008efe70:      mov x0, x19
1008efe74:      mov x1, x20
1008efe78:      mov w2, #0x1                ; =1
1008efe7c:      mov w3, #0x1                ; =1
1008efe80:      mov w4, #0x1                ; =1
1008efe84:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008efe88:      mov x0, x22
1008efe8c:      b   0x1008efd18 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x78>
1008efe90:      mov x0, x19
1008efe94:      mov x1, x22
1008efe98:      mov w2, #0x1                ; =1
1008efe9c:      mov w3, #0x1                ; =1
1008efea0:      mov w4, #0x1                ; =1
1008efea4:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008efea8:      b   0x1008efd64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xc4>
1008efeac:      adrp    x0, 0x100e1b000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1432+0x9>
1008efeb0:      add x0, x0, #0x990
1008efeb4:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008efeb8:      add x2, x2, #0xf78
1008efebc:      mov w1, #0x30               ; =48
1008efec0:      bl  0x100c9e128 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
