/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/short-tail-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008dca80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record>:
1008dca80:      stp x28, x27, [sp, #-0x60]!
1008dca84:      stp x26, x25, [sp, #0x10]
1008dca88:      stp x24, x23, [sp, #0x20]
1008dca8c:      stp x22, x21, [sp, #0x30]
1008dca90:      stp x20, x19, [sp, #0x40]
1008dca94:      stp x29, x30, [sp, #0x50]
1008dca98:      add x29, sp, #0x50
1008dca9c:      sub sp, sp, #0x5e0
1008dcaa0:      ldr xzr, [sp]
1008dcaa4:      str x1, [sp, #0x38]
1008dcaa8:      mov x10, x0
1008dcaac:      movi.2d v0, #0000000000000000
1008dcab0:      str d0, [sp, #0x40]
1008dcab4:      str wzr, [sp, #0x48]
1008dcab8:      str d0, [sp, #0x68]
1008dcabc:      str wzr, [sp, #0x70]
1008dcac0:      str d0, [sp, #0x90]
1008dcac4:      str wzr, [sp, #0x98]
1008dcac8:      str d0, [sp, #0xb8]
1008dcacc:      str wzr, [sp, #0xc0]
1008dcad0:      str d0, [sp, #0xe0]
1008dcad4:      str wzr, [sp, #0xe8]
1008dcad8:      str d0, [sp, #0x108]
1008dcadc:      str wzr, [sp, #0x110]
1008dcae0:      str d0, [sp, #0x130]
1008dcae4:      str wzr, [sp, #0x138]
1008dcae8:      str d0, [sp, #0x158]
1008dcaec:      str wzr, [sp, #0x160]
1008dcaf0:      str d0, [sp, #0x180]
1008dcaf4:      str wzr, [sp, #0x188]
1008dcaf8:      str d0, [sp, #0x1a8]
1008dcafc:      str wzr, [sp, #0x1b0]
1008dcb00:      str d0, [sp, #0x1d0]
1008dcb04:      str wzr, [sp, #0x1d8]
1008dcb08:      str d0, [sp, #0x1f8]
1008dcb0c:      str wzr, [sp, #0x200]
1008dcb10:      str d0, [sp, #0x220]
1008dcb14:      str wzr, [sp, #0x228]
1008dcb18:      str d0, [sp, #0x248]
1008dcb1c:      str wzr, [sp, #0x250]
1008dcb20:      str d0, [sp, #0x270]
1008dcb24:      str wzr, [sp, #0x278]
1008dcb28:      str d0, [sp, #0x298]
1008dcb2c:      str wzr, [sp, #0x2a0]
1008dcb30:      str d0, [sp, #0x2c0]
1008dcb34:      str wzr, [sp, #0x2c8]
1008dcb38:      str d0, [sp, #0x2e8]
1008dcb3c:      str wzr, [sp, #0x2f0]
1008dcb40:      str d0, [sp, #0x310]
1008dcb44:      str wzr, [sp, #0x318]
1008dcb48:      str d0, [sp, #0x338]
1008dcb4c:      str wzr, [sp, #0x340]
1008dcb50:      str d0, [sp, #0x360]
1008dcb54:      str wzr, [sp, #0x368]
1008dcb58:      str d0, [sp, #0x388]
1008dcb5c:      str wzr, [sp, #0x390]
1008dcb60:      str d0, [sp, #0x3b0]
1008dcb64:      str wzr, [sp, #0x3b8]
1008dcb68:      str d0, [sp, #0x3d8]
1008dcb6c:      str wzr, [sp, #0x3e0]
1008dcb70:      str d0, [sp, #0x400]
1008dcb74:      str wzr, [sp, #0x408]
1008dcb78:      str d0, [sp, #0x428]
1008dcb7c:      str wzr, [sp, #0x430]
1008dcb80:      str d0, [sp, #0x450]
1008dcb84:      str wzr, [sp, #0x458]
1008dcb88:      str d0, [sp, #0x478]
1008dcb8c:      str wzr, [sp, #0x480]
1008dcb90:      str d0, [sp, #0x4a0]
1008dcb94:      str wzr, [sp, #0x4a8]
1008dcb98:      str d0, [sp, #0x4c8]
1008dcb9c:      str wzr, [sp, #0x4d0]
1008dcba0:      str d0, [sp, #0x4f0]
1008dcba4:      str wzr, [sp, #0x4f8]
1008dcba8:      str d0, [sp, #0x518]
1008dcbac:      str wzr, [sp, #0x520]
1008dcbb0:      ldr w19, [x0, #0x4]
1008dcbb4:      adrp    x8, 0x101121000 <__MergedGlobals>
1008dcbb8:      add x8, x8, #0xcc4
1008dcbbc:      ldr w20, [x8]
1008dcbc0:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1008dcbc4:      add x8, x8, #0x980
1008dcbc8:      cmp w20, #0x300
1008dcbcc:      b.hs    0x1008dcc64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1008dcbd0:      ldr x8, [x8]
1008dcbd4:      cmn x8, #0x1
1008dcbd8:      b.eq    0x1008dcc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
1008dcbdc:      mrs x9, TPIDRRO_EL0
1008dcbe0:      and x9, x9, #0xfffffffffffffff8
1008dcbe4:      ldr x0, [x9, x8, lsl #3]
1008dcbe8:      cbz x0, 0x1008dcc4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
1008dcbec:      add x8, x0, x20, lsl #3
1008dcbf0:      ldr x0, [x8, #0x1e8]
1008dcbf4:      cbz x0, 0x1008dcc64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1008dcbf8:      ldr x0, [x0]
1008dcbfc:      cbz x0, 0x1008dcc80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x200>
1008dcc00:      mov w8, #-0x40000001        ; =-1073741825
1008dcc04:      cmp w19, w8
1008dcc08:      b.gt    0x1008dcc98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
1008dcc0c:      ldr x9, [x0, #0x5198]
1008dcc10:      ubfx    x8, x19, #15, #15
1008dcc14:      cmp x8, x9
1008dcc18:      b.hs    0x1008dcc98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
1008dcc1c:      ldr x9, [x0, #0x5190]
1008dcc20:      ldr x8, [x9, x8, lsl #3]
1008dcc24:      cbz x8, 0x1008dcc9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1008dcc28:      ubfx    x9, x19, #5, #10
1008dcc2c:      ldr x8, [x8, x9, lsl #3]
1008dcc30:      cbz x8, 0x1008dcc9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1008dcc34:      and x9, x19, #0x1f
1008dcc38:      add x8, x8, x9, lsl #5
1008dcc3c:      ldrb    w9, [x8, #0x1c]
1008dcc40:      tbz w9, #0x0, 0x1008dcc98 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
1008dcc44:      ldr x8, [x8]
1008dcc48:      b   0x1008dcc9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1008dcc4c:      mov x21, x10
1008dcc50:      bl  0x100c8a348 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008dcc54:      mov x10, x21
1008dcc58:      add x8, x0, x20, lsl #3
1008dcc5c:      ldr x0, [x8, #0x1e8]
1008dcc60:      cbnz    x0, 0x1008dcbf8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x178>
1008dcc64:      adrp    x0, 0x1010c8000 <_anon.534fe2791366adc85564f9268bfdb267.1230+0x448>
1008dcc68:      add x0, x0, #0xd18
1008dcc6c:      mov x20, x10
1008dcc70:      bl  0x100c89a58 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008dcc74:      mov x10, x20
1008dcc78:      ldr x0, [x0]
1008dcc7c:      cbnz    x0, 0x1008dcc00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x180>
1008dcc80:      mov x20, x10
1008dcc84:      bl  0x100cbd098 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008dcc88:      mov x10, x20
1008dcc8c:      mov w8, #-0x40000001        ; =-1073741825
1008dcc90:      cmp w19, w8
1008dcc94:      b.le    0x1008dcc0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x18c>
1008dcc98:      mov x8, #0x0                ; =0
1008dcc9c:      mov x23, #0x0               ; =0
1008dcca0:      add x8, x8, #0x8
1008dcca4:      str x8, [sp, #0x30]
1008dcca8:      sub x28, x29, #0x80
1008dccac:      add x8, x10, #0x10
1008dccb0:      stp xzr, x8, [sp, #0x20]
1008dccb4:      add x8, sp, #0x2c0
1008dccb8:      add x8, x8, #0x8
1008dccbc:      stp x10, x8, [sp, #0x10]
1008dccc0:      mov w22, #0x2               ; =2
1008dccc4:      mov w27, #0x28              ; =40
1008dccc8:      mov w26, #0x2               ; =2
1008dcccc:      ldr x8, [sp, #0x30]
1008dccd0:      ldr x1, [x8, x23, lsl #3]
1008dccd4:      sub x0, x29, #0x80
1008dccd8:      bl  0x1008d738c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
1008dccdc:      ldur    w9, [x29, #-0x80]
1008dcce0:      cmn w9, #0x1
1008dcce4:      b.eq    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcce8:      ldp w8, w19, [x29, #-0x7c]
1008dccec:      ldur    w25, [x29, #-0x74]
1008dccf0:      ldur    q0, [x28, #0x10]
1008dccf4:      stur    q0, [x29, #-0xf0]
1008dccf8:      ldur    x10, [x28, #0x20]
1008dccfc:      stur    x10, [x29, #-0xe0]
1008dcd00:      add x11, sp, #0x40
1008dcd04:      madd    x11, x23, x27, x11
1008dcd08:      stp w9, w8, [x11]
1008dcd0c:      stp w19, w25, [x11, #0x8]
1008dcd10:      str q0, [x11, #0x10]
1008dcd14:      str x10, [x11, #0x20]
1008dcd18:      cmp w9, #0x2
1008dcd1c:      b.eq    0x1008dcd34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2b4>
1008dcd20:      cmp w9, #0x1
1008dcd24:      b.eq    0x1008dcd3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
1008dcd28:      add w25, w19, #0x2
1008dcd2c:      add w19, w8, #0x2
1008dcd30:      b   0x1008dcd3c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
1008dcd34:      mov x25, x8
1008dcd38:      mov x19, x8
1008dcd3c:      ldr x8, [sp, #0x28]
1008dcd40:      ldr x21, [x8, x23, lsl #3]
1008dcd44:      sub x0, x29, #0xd8
1008dcd48:      mov x1, x21
1008dcd4c:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008dcd50:      ldur    w8, [x29, #-0xd8]
1008dcd54:      cmn w8, #0x1
1008dcd58:      b.eq    0x1008dcdb4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x334>
1008dcd5c:      ldp w20, w21, [x29, #-0xd4]
1008dcd60:      ldur    w9, [x29, #-0xcc]
1008dcd64:      add x10, sp, #0x180
1008dcd68:      madd    x10, x23, x27, x10
1008dcd6c:      stp w8, w20, [x10]
1008dcd70:      stp w21, w9, [x10, #0x8]
1008dcd74:      sub x11, x29, #0xd8
1008dcd78:      ldur    q0, [x11, #0x10]
1008dcd7c:      str q0, [x10, #0x10]
1008dcd80:      ldur    x11, [x11, #0x20]
1008dcd84:      str x11, [x10, #0x20]
1008dcd88:      cmp w8, #0x2
1008dcd8c:      b.eq    0x1008dcf64 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x4e4>
1008dcd90:      cmp w8, #0x1
1008dcd94:      b.ne    0x1008dcf80 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x500>
1008dcd98:      mov x20, x9
1008dcd9c:      cmp x23, #0x0
1008dcda0:      mov w8, #0x1                ; =1
1008dcda4:      cinc    w8, w8, ne
1008dcda8:      adds    w9, w19, w22
1008dcdac:      b.lo    0x1008dcfc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1008dcdb0:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcdb4:      mov w8, #0x7ffd             ; =32765
1008dcdb8:      cmp x8, x21, lsr #48
1008dcdbc:      b.ne    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcdc0:      and x21, x21, #0xffffffffffff
1008dcdc4:      mov x0, x21
1008dcdc8:      bl  0x1008fc174 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008dcdcc:      cbz x0, 0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dcdd0:      ldrb    w8, [x0]
1008dcdd4:      cmp w8, #0x1
1008dcdd8:      b.ne    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcddc:      ldrsb   w8, [x0, #0x1]
1008dcde0:      tbnz    w8, #0x1f, 0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcde4:      ldrh    w8, [x0, #0x2]
1008dcde8:      tbnz    w8, #0xa, 0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcdec:      ldr w8, [x0, #0x4]
1008dcdf0:      cmp w8, #0x10
1008dcdf4:      b.lo    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcdf8:      ldr w20, [x21]
1008dcdfc:      cmp w20, #0x10
1008dce00:      b.hi    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce04:      ldr w9, [x21, #0x4]
1008dce08:      cmp w20, w9
1008dce0c:      b.hi    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce10:      lsl x24, x20, #3
1008dce14:      add x9, x24, #0x10
1008dce18:      cmp x9, x8
1008dce1c:      b.hi    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce20:      mov x0, x21
1008dce24:      bl  0x1008f9924 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
1008dce28:      tbnz    w0, #0x0, 0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce2c:      mov x0, x21
1008dce30:      bl  0x1001d1ac0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
1008dce34:      cmp x0, #0x1
1008dce38:      b.eq    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce3c:      ldr x8, [sp, #0x20]
1008dce40:      add x10, x8, x20
1008dce44:      cmp x10, #0x10
1008dce48:      b.hi    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dce4c:      add x8, sp, #0x180
1008dce50:      madd    x8, x23, x27, x8
1008dce54:      mov w9, #-0x1               ; =-1
1008dce58:      str w9, [x8]
1008dce5c:      ldr x9, [sp, #0x20]
1008dce60:      stp x9, x20, [x8, #0x8]
1008dce64:      cbz w20, 0x1008dcfa4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x524>
1008dce68:      str x10, [sp]
1008dce6c:      stp w26, w22, [sp, #0x8]
1008dce70:      mov x22, #0x0               ; =0
1008dce74:      mov x10, x9
1008dce78:      mov w9, #0x28               ; =40
1008dce7c:      add x27, x21, #0x8
1008dce80:      ldr x8, [sp, #0x18]
1008dce84:      madd    x26, x10, x9, x8
1008dce88:      mov w20, #0x2               ; =2
1008dce8c:      mov w21, #0x2               ; =2
1008dce90:      ldr x1, [x27, x22]
1008dce94:      sub x0, x29, #0x80
1008dce98:      bl  0x1008d70c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1008dce9c:      ldur    w8, [x29, #-0x80]
1008dcea0:      cmn w8, #0x1
1008dcea4:      b.eq    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcea8:      ldp w10, w9, [x29, #-0x7c]
1008dceac:      ldur    w11, [x29, #-0x74]
1008dceb0:      ldur    q0, [x28, #0x10]
1008dceb4:      stur    q0, [x29, #-0xb0]
1008dceb8:      ldur    x12, [x28, #0x20]
1008dcebc:      stur    x12, [x29, #-0xa0]
1008dcec0:      stp w8, w10, [x26, #-0x8]
1008dcec4:      stp w9, w11, [x26]
1008dcec8:      stur    q0, [x26, #0x8]
1008dcecc:      str x12, [x26, #0x18]
1008dced0:      cbz w8, 0x1008dcef4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x474>
1008dced4:      cmp w8, #0x1
1008dced8:      csel    w8, w11, w10, eq
1008dcedc:      csel    w10, w9, w10, eq
1008dcee0:      cmp x22, #0x0
1008dcee4:      cset    w9, ne
1008dcee8:      adds    w10, w10, w21
1008dceec:      b.lo    0x1008dcf0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x48c>
1008dcef0:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcef4:      add w10, w10, #0x2
1008dcef8:      add w8, w9, #0x2
1008dcefc:      cmp x22, #0x0
1008dcf00:      cset    w9, ne
1008dcf04:      adds    w10, w10, w21
1008dcf08:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcf0c:      adds    w21, w10, w9
1008dcf10:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcf14:      mov x0, #0x0                ; =0
1008dcf18:      adds    w8, w8, w20
1008dcf1c:      cset    w10, hs
1008dcf20:      adds    w20, w8, w9
1008dcf24:      cset    w8, hs
1008dcf28:      tbnz    w10, #0x0, 0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dcf2c:      tbnz    w8, #0x0, 0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dcf30:      add x22, x22, #0x8
1008dcf34:      add x26, x26, #0x28
1008dcf38:      cmp x24, x22
1008dcf3c:      b.ne    0x1008dce90 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x410>
1008dcf40:      ldr x8, [sp]
1008dcf44:      str x8, [sp, #0x20]
1008dcf48:      ldp w26, w22, [sp, #0x8]
1008dcf4c:      cmp x23, #0x0
1008dcf50:      mov w8, #0x1                ; =1
1008dcf54:      cinc    w8, w8, ne
1008dcf58:      adds    w9, w19, w22
1008dcf5c:      b.lo    0x1008dcfc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1008dcf60:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcf64:      mov x21, x20
1008dcf68:      cmp x23, #0x0
1008dcf6c:      mov w8, #0x1                ; =1
1008dcf70:      cinc    w8, w8, ne
1008dcf74:      adds    w9, w19, w22
1008dcf78:      b.lo    0x1008dcfc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1008dcf7c:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcf80:      add w8, w20, #0x2
1008dcf84:      add w20, w21, #0x2
1008dcf88:      mov x21, x8
1008dcf8c:      cmp x23, #0x0
1008dcf90:      mov w8, #0x1                ; =1
1008dcf94:      cinc    w8, w8, ne
1008dcf98:      adds    w9, w19, w22
1008dcf9c:      b.lo    0x1008dcfc4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1008dcfa0:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcfa4:      mov w20, #0x2               ; =2
1008dcfa8:      mov w21, #0x2               ; =2
1008dcfac:      str x10, [sp, #0x20]
1008dcfb0:      cmp x23, #0x0
1008dcfb4:      mov w8, #0x1                ; =1
1008dcfb8:      cinc    w8, w8, ne
1008dcfbc:      adds    w9, w19, w22
1008dcfc0:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcfc4:      adds    w9, w21, w9
1008dcfc8:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcfcc:      adds    w11, w9, w8
1008dcfd0:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcfd4:      adds    w9, w25, w26
1008dcfd8:      b.hs    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dcfdc:      mov x0, #0x0                ; =0
1008dcfe0:      adds    w9, w20, w9
1008dcfe4:      cset    w10, hs
1008dcfe8:      adds    w24, w9, w8
1008dcfec:      cset    w8, hs
1008dcff0:      tbnz    w10, #0x0, 0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dcff4:      tbnz    w8, #0x0, 0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dcff8:      mov x20, x11
1008dcffc:      add x23, x23, #0x1
1008dd000:      ldr x8, [sp, #0x38]
1008dd004:      cmp x23, x8
1008dd008:      mov x22, x11
1008dd00c:      mov x26, x24
1008dd010:      mov w27, #0x28              ; =40
1008dd014:      b.ne    0x1008dcccc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x24c>
1008dd018:      adrp    x19, 0x101120000 <_perry_global_baseline_worker_ts__1>
1008dd01c:      add x19, x19, #0x980
1008dd020:      ldr x8, [x19]
1008dd024:      cmn x8, #0x1
1008dd028:      b.eq    0x1008dd05c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
1008dd02c:      mrs x9, TPIDRRO_EL0
1008dd030:      and x9, x9, #0xfffffffffffffff8
1008dd034:      ldr x8, [x9, x8, lsl #3]
1008dd038:      cbz x8, 0x1008dd05c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
1008dd03c:      ldr x8, [x8, #0x19e8]
1008dd040:      cbz x8, 0x1008dd090 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x610>
1008dd044:      ldr x9, [x8]
1008dd048:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008dd04c:      cmp x9, x10
1008dd050:      b.hs    0x1008dd630 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbb0>
1008dd054:      ldr x21, [x8, #0x18]
1008dd058:      b   0x1008dd0a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x620>
1008dd05c:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008dd060:      add x0, x0, #0x8f8
1008dd064:      ldr x8, [x0]
1008dd068:      blr x8
1008dd06c:      ldrb    w8, [x0, #0x20]
1008dd070:      cbnz    w8, 0x1008dd5d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb58>
1008dd074:      ldr x8, [x0]
1008dd078:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008dd07c:      cmp x8, x9
1008dd080:      ldr x20, [sp, #0x10]
1008dd084:      b.hs    0x1008dd614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb94>
1008dd088:      ldr x21, [x0, #0x18]
1008dd08c:      b   0x1008dd0a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x624>
1008dd090:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd094:      add x0, x0, #0x600
1008dd098:      bl  0x10013546c <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1008dd09c:      mov x21, x0
1008dd0a0:      ldr x20, [sp, #0x10]
1008dd0a4:      stur    x21, [x29, #-0x90]
1008dd0a8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1008dd0ac:      stp x20, x8, [x29, #-0x78]
1008dd0b0:      mov w8, #0x1                ; =1
1008dd0b4:      stur    x8, [x29, #-0x80]
1008dd0b8:      sub x0, x29, #0x80
1008dd0bc:      bl  0x10089a0c8 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1008dd0c0:      mov x24, x0
1008dd0c4:      stur    x0, [x29, #-0x88]
1008dd0c8:      adrp    x0, 0x101128000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10perf_hooks13LOOP_START_MS0s_023___RUST_STD_INTERNAL_VAL>
1008dd0cc:      add x0, x0, #0x5e8
1008dd0d0:      ldr x8, [x0]
1008dd0d4:      blr x8
1008dd0d8:      strb    wzr, [x0]
1008dd0dc:      mov x0, x20
1008dd0e0:      bl  0x1002da440 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1008dd0e4:      tbz w0, #0x0, 0x1008dd188 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x708>
1008dd0e8:      mov x0, x22
1008dd0ec:      bl  0x1008a726c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
1008dd0f0:      mov x23, x1
1008dd0f4:      stp w26, w22, [x0]
1008dd0f8:      stp wzr, wzr, [x0, #0xc]
1008dd0fc:      str w22, [x0, #0x8]
1008dd100:      ldr x8, [x19]
1008dd104:      cmn x8, #0x1
1008dd108:      str x0, [sp, #0x20]
1008dd10c:      b.eq    0x1008dd1cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
1008dd110:      mrs x9, TPIDRRO_EL0
1008dd114:      and x9, x9, #0xfffffffffffffff8
1008dd118:      ldr x8, [x9, x8, lsl #3]
1008dd11c:      cbz x8, 0x1008dd1cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
1008dd120:      ldr x8, [x8, #0x19e8]
1008dd124:      cbz x8, 0x1008dd2fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x87c>
1008dd128:      ldr x9, [x8]
1008dd12c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1008dd130:      cmp x9, x10
1008dd134:      b.hs    0x1008dd6c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc44>
1008dd138:      add x10, x9, #0x1
1008dd13c:      str x10, [x8]
1008dd140:      ldr x10, [x8, #0x18]
1008dd144:      cmp x24, x10
1008dd148:      b.hs    0x1008dd5d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
1008dd14c:      ldr x10, [x8, #0x10]
1008dd150:      mov w11, #0x18              ; =24
1008dd154:      madd    x10, x24, x11, x10
1008dd158:      ldr x11, [x10]
1008dd15c:      cmp x11, #0x1
1008dd160:      b.ne    0x1008dd6d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc50>
1008dd164:      ldr x22, [x10, #0x8]
1008dd168:      str x9, [x8]
1008dd16c:      ldr w19, [x22, #0x4]
1008dd170:      adrp    x8, 0x101121000 <__MergedGlobals>
1008dd174:      add x8, x8, #0xcc4
1008dd178:      ldr w20, [x8]
1008dd17c:      cmp w20, #0x300
1008dd180:      b.lo    0x1008dd240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
1008dd184:      b   0x1008dd370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1008dd188:      ldr x8, [x19]
1008dd18c:      cmn x8, #0x1
1008dd190:      b.eq    0x1008dd2c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
1008dd194:      mrs x9, TPIDRRO_EL0
1008dd198:      and x9, x9, #0xfffffffffffffff8
1008dd19c:      ldr x8, [x9, x8, lsl #3]
1008dd1a0:      cbz x8, 0x1008dd2c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
1008dd1a4:      ldr x8, [x8, #0x19e8]
1008dd1a8:      cbz x8, 0x1008dd32c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8ac>
1008dd1ac:      ldr x9, [x8]
1008dd1b0:      cbnz    x9, 0x1008dd63c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
1008dd1b4:      ldr x9, [x8, #0x18]
1008dd1b8:      cmp x21, x9
1008dd1bc:      b.hi    0x1008dd1c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x744>
1008dd1c0:      str x21, [x8, #0x18]
1008dd1c4:      str xzr, [x8]
1008dd1c8:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dd1cc:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008dd1d0:      add x0, x0, #0x8f8
1008dd1d4:      ldr x8, [x0]
1008dd1d8:      blr x8
1008dd1dc:      ldrb    w8, [x0, #0x20]
1008dd1e0:      cbnz    w8, 0x1008dd648 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbc8>
1008dd1e4:      ldr x8, [x0]
1008dd1e8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008dd1ec:      cmp x8, x9
1008dd1f0:      b.hs    0x1008dd678 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbf8>
1008dd1f4:      add x9, x8, #0x1
1008dd1f8:      str x9, [x0]
1008dd1fc:      ldr x9, [x0, #0x18]
1008dd200:      cmp x24, x9
1008dd204:      b.hs    0x1008dd5d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
1008dd208:      ldr x9, [x0, #0x10]
1008dd20c:      mov w10, #0x18              ; =24
1008dd210:      madd    x9, x24, x10, x9
1008dd214:      ldr x10, [x9]
1008dd218:      cmp x10, #0x1
1008dd21c:      b.ne    0x1008dd620 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xba0>
1008dd220:      ldr x22, [x9, #0x8]
1008dd224:      str x8, [x0]
1008dd228:      ldr w19, [x22, #0x4]
1008dd22c:      adrp    x8, 0x101121000 <__MergedGlobals>
1008dd230:      add x8, x8, #0xcc4
1008dd234:      ldr w20, [x8]
1008dd238:      cmp w20, #0x300
1008dd23c:      b.hs    0x1008dd370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1008dd240:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1008dd244:      add x8, x8, #0x980
1008dd248:      ldr x8, [x8]
1008dd24c:      cmn x8, #0x1
1008dd250:      b.eq    0x1008dd360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
1008dd254:      mrs x9, TPIDRRO_EL0
1008dd258:      and x9, x9, #0xfffffffffffffff8
1008dd25c:      ldr x0, [x9, x8, lsl #3]
1008dd260:      cbz x0, 0x1008dd360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
1008dd264:      add x8, x0, x20, lsl #3
1008dd268:      ldr x0, [x8, #0x1e8]
1008dd26c:      cbz x0, 0x1008dd370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1008dd270:      ldr x0, [x0]
1008dd274:      cbz x0, 0x1008dd384 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x904>
1008dd278:      mov w8, #-0x40000001        ; =-1073741825
1008dd27c:      cmp w19, w8
1008dd280:      str x21, [sp, #0x18]
1008dd284:      b.gt    0x1008dd398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
1008dd288:      ldr x9, [x0, #0x5198]
1008dd28c:      ubfx    x8, x19, #15, #15
1008dd290:      cmp x8, x9
1008dd294:      b.hs    0x1008dd398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
1008dd298:      ldr x9, [x0, #0x5190]
1008dd29c:      ldr x8, [x9, x8, lsl #3]
1008dd2a0:      cbz x8, 0x1008dd39c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1008dd2a4:      ubfx    x9, x19, #5, #10
1008dd2a8:      ldr x8, [x8, x9, lsl #3]
1008dd2ac:      cbz x8, 0x1008dd39c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1008dd2b0:      and x9, x19, #0x1f
1008dd2b4:      add x8, x8, x9, lsl #5
1008dd2b8:      ldrb    w9, [x8, #0x1c]
1008dd2bc:      tbz w9, #0x0, 0x1008dd398 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
1008dd2c0:      ldr x8, [x8]
1008dd2c4:      b   0x1008dd39c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1008dd2c8:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008dd2cc:      add x0, x0, #0x8f8
1008dd2d0:      ldr x8, [x0]
1008dd2d4:      blr x8
1008dd2d8:      ldrb    w8, [x0, #0x20]
1008dd2dc:      cbnz    w8, 0x1008dd684 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc04>
1008dd2e0:      ldr x8, [x0]
1008dd2e4:      cbnz    x8, 0x1008dd700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
1008dd2e8:      ldr x8, [x0, #0x18]
1008dd2ec:      cmp x21, x8
1008dd2f0:      b.hi    0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dd2f4:      str x21, [x0, #0x18]
1008dd2f8:      b   0x1008dd33c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1008dd2fc:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd300:      add x0, x0, #0x600
1008dd304:      sub x1, x29, #0x88
1008dd308:      bl  0x100135290 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
1008dd30c:      mov x22, x0
1008dd310:      ldr w19, [x0, #0x4]
1008dd314:      adrp    x8, 0x101121000 <__MergedGlobals>
1008dd318:      add x8, x8, #0xcc4
1008dd31c:      ldr w20, [x8]
1008dd320:      cmp w20, #0x300
1008dd324:      b.lo    0x1008dd240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
1008dd328:      b   0x1008dd370 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1008dd32c:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd330:      add x0, x0, #0x600
1008dd334:      sub x1, x29, #0x90
1008dd338:      bl  0x100135848 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1008dd33c:      mov x0, #0x0                ; =0
1008dd340:      add sp, sp, #0x5e0
1008dd344:      ldp x29, x30, [sp, #0x50]
1008dd348:      ldp x20, x19, [sp, #0x40]
1008dd34c:      ldp x22, x21, [sp, #0x30]
1008dd350:      ldp x24, x23, [sp, #0x20]
1008dd354:      ldp x26, x25, [sp, #0x10]
1008dd358:      ldp x28, x27, [sp], #0x60
1008dd35c:      ret
1008dd360:      bl  0x100c8a348 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008dd364:      add x8, x0, x20, lsl #3
1008dd368:      ldr x0, [x8, #0x1e8]
1008dd36c:      cbnz    x0, 0x1008dd270 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f0>
1008dd370:      adrp    x0, 0x1010c8000 <_anon.534fe2791366adc85564f9268bfdb267.1230+0x448>
1008dd374:      add x0, x0, #0xd18
1008dd378:      bl  0x100c89a58 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008dd37c:      ldr x0, [x0]
1008dd380:      cbnz    x0, 0x1008dd278 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f8>
1008dd384:      bl  0x100cbd098 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008dd388:      mov w8, #-0x40000001        ; =-1073741825
1008dd38c:      cmp w19, w8
1008dd390:      str x21, [sp, #0x18]
1008dd394:      b.le    0x1008dd288 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x808>
1008dd398:      mov x8, #0x0                ; =0
1008dd39c:      mov x19, #0x0               ; =0
1008dd3a0:      mov w9, #0x7b               ; =123
1008dd3a4:      strb    w9, [x23]
1008dd3a8:      add x26, x8, #0x8
1008dd3ac:      add x25, x22, #0x10
1008dd3b0:      add x8, sp, #0x2c0
1008dd3b4:      add x8, x8, #0x28
1008dd3b8:      stp x8, x26, [sp, #0x28]
1008dd3bc:      mov w21, #0x1               ; =1
1008dd3c0:      add x27, sp, #0x40
1008dd3c4:      mov w28, #0x3a              ; =58
1008dd3c8:      mov w20, #0x2c              ; =44
1008dd3cc:      b   0x1008dd3f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
1008dd3d0:      mov w8, #0x5d               ; =93
1008dd3d4:      strb    w8, [x23, x26]
1008dd3d8:      add x21, x26, #0x1
1008dd3dc:      ldp x26, x8, [sp, #0x30]
1008dd3e0:      add x27, sp, #0x40
1008dd3e4:      mov w28, #0x3a              ; =58
1008dd3e8:      add x19, x19, #0x1
1008dd3ec:      cmp x19, x8
1008dd3f0:      b.eq    0x1008dd520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
1008dd3f4:      cbz x19, 0x1008dd400 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x980>
1008dd3f8:      strb    w20, [x23, x21]
1008dd3fc:      add x21, x21, #0x1
1008dd400:      add x8, x19, x19, lsl #2
1008dd404:      lsl x24, x8, #3
1008dd408:      ldr x1, [x26, x19, lsl #3]
1008dd40c:      add x0, x27, x24
1008dd410:      add x2, x23, x21
1008dd414:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008dd418:      add x8, x0, x21
1008dd41c:      strb    w28, [x23, x8]
1008dd420:      add x22, x8, #0x1
1008dd424:      ldr x1, [x25, x19, lsl #3]
1008dd428:      add x9, sp, #0x180
1008dd42c:      add x0, x9, x24
1008dd430:      ldr w9, [x0]
1008dd434:      cmn w9, #0x1
1008dd438:      b.eq    0x1008dd45c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x9dc>
1008dd43c:      add x2, x23, x22
1008dd440:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008dd444:      add x21, x0, x22
1008dd448:      add x19, x19, #0x1
1008dd44c:      ldr x8, [sp, #0x38]
1008dd450:      cmp x19, x8
1008dd454:      b.ne    0x1008dd3f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
1008dd458:      b   0x1008dd520 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
1008dd45c:      ldp x24, x21, [x0, #0x8]
1008dd460:      add x26, x8, #0x2
1008dd464:      mov w8, #0x5b               ; =91
1008dd468:      strb    w8, [x23, x22]
1008dd46c:      cbz x21, 0x1008dd3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1008dd470:      cmp x24, #0xf
1008dd474:      b.hi    0x1008dd70c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc8c>
1008dd478:      and x22, x1, #0xffffffffffff
1008dd47c:      add x8, sp, #0x2c0
1008dd480:      mov w9, #0x28               ; =40
1008dd484:      madd    x8, x24, x9, x8
1008dd488:      ldp q0, q1, [x8]
1008dd48c:      stp q0, q1, [x29, #-0x80]
1008dd490:      ldr x8, [x8, #0x20]
1008dd494:      stur    x8, [x29, #-0x60]
1008dd498:      ldr x1, [x22, #0x8]
1008dd49c:      sub x0, x29, #0x80
1008dd4a0:      add x2, x23, x26
1008dd4a4:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008dd4a8:      add x26, x0, x26
1008dd4ac:      subs    x28, x21, #0x1
1008dd4b0:      b.eq    0x1008dd3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1008dd4b4:      add x21, x22, #0x10
1008dd4b8:      cmp x24, #0x10
1008dd4bc:      mov w8, #0x10               ; =16
1008dd4c0:      csel    x8, x24, x8, lo
1008dd4c4:      mov w9, #0x28               ; =40
1008dd4c8:      sub x27, x8, #0xf
1008dd4cc:      add x22, x24, #0x1
1008dd4d0:      ldr x8, [sp, #0x28]
1008dd4d4:      madd    x24, x24, x9, x8
1008dd4d8:      strb    w20, [x23, x26]
1008dd4dc:      cbz x27, 0x1008dd710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc90>
1008dd4e0:      add x26, x26, #0x1
1008dd4e4:      ldp q0, q1, [x24]
1008dd4e8:      stp q0, q1, [x29, #-0x80]
1008dd4ec:      ldr x8, [x24, #0x20]
1008dd4f0:      stur    x8, [x29, #-0x60]
1008dd4f4:      ldr x1, [x21], #0x8
1008dd4f8:      sub x0, x29, #0x80
1008dd4fc:      add x2, x23, x26
1008dd500:      bl  0x1008d62a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1008dd504:      add x26, x0, x26
1008dd508:      add x27, x27, #0x1
1008dd50c:      add x22, x22, #0x1
1008dd510:      add x24, x24, #0x28
1008dd514:      subs    x28, x28, #0x1
1008dd518:      b.ne    0x1008dd4d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xa58>
1008dd51c:      b   0x1008dd3d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1008dd520:      mov w8, #0x7d               ; =125
1008dd524:      strb    w8, [x23, x21]
1008dd528:      adrp    x8, 0x101120000 <_perry_global_baseline_worker_ts__1>
1008dd52c:      add x8, x8, #0x980
1008dd530:      ldr x8, [x8]
1008dd534:      cmn x8, #0x1
1008dd538:      b.eq    0x1008dd578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
1008dd53c:      mrs x9, TPIDRRO_EL0
1008dd540:      and x9, x9, #0xfffffffffffffff8
1008dd544:      ldr x8, [x9, x8, lsl #3]
1008dd548:      cbz x8, 0x1008dd578 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
1008dd54c:      ldr x8, [x8, #0x19e8]
1008dd550:      ldr x10, [sp, #0x18]
1008dd554:      cbz x8, 0x1008dd5b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb30>
1008dd558:      ldr x9, [x8]
1008dd55c:      cbnz    x9, 0x1008dd63c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
1008dd560:      ldr x9, [x8, #0x18]
1008dd564:      cmp x10, x9
1008dd568:      b.hi    0x1008dd570 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf0>
1008dd56c:      str x10, [x8, #0x18]
1008dd570:      str xzr, [x8]
1008dd574:      b   0x1008dd5c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1008dd578:      adrp    x0, 0x101127000 <__RNvNCNKNvNvNtCs5gMwpk3Cs4e_13perry_runtime3set17SET_FOREACH_STACK7STORAGE0023___RUST_STD_INTERNAL_VAL+0x8>
1008dd57c:      add x0, x0, #0x8f8
1008dd580:      ldr x8, [x0]
1008dd584:      blr x8
1008dd588:      ldrb    w8, [x0, #0x20]
1008dd58c:      ldr x20, [sp, #0x18]
1008dd590:      cbnz    w8, 0x1008dd6b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc30>
1008dd594:      ldr x8, [x0]
1008dd598:      cbnz    x8, 0x1008dd700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
1008dd59c:      ldr x8, [x0, #0x18]
1008dd5a0:      cmp x20, x8
1008dd5a4:      b.hi    0x1008dd5c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1008dd5a8:      str x20, [x0, #0x18]
1008dd5ac:      b   0x1008dd5c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1008dd5b0:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd5b4:      add x0, x0, #0x600
1008dd5b8:      sub x1, x29, #0x90
1008dd5bc:      bl  0x100135848 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
1008dd5c0:      mov x1, #0x7fff000000000000 ; =9223090561878065152
1008dd5c4:      ldr x8, [sp, #0x20]
1008dd5c8:      bfxil   x1, x8, #0, #48
1008dd5cc:      mov w0, #0x1                ; =1
1008dd5d0:      b   0x1008dd340 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1008dd5d4:      bl  0x100cb1470 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1008dd5d8:      cmp w8, #0x1
1008dd5dc:      b.ne    0x1008dd6b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
1008dd5e0:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008dd5e4:      add x1, x1, #0xc0
1008dd5e8:      mov x21, x0
1008dd5ec:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008dd5f0:      mov x0, x21
1008dd5f4:      strb    wzr, [x21, #0x20]
1008dd5f8:      mov x22, x20
1008dd5fc:      mov x26, x24
1008dd600:      ldr x8, [x21]
1008dd604:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008dd608:      cmp x8, x9
1008dd60c:      ldr x20, [sp, #0x10]
1008dd610:      b.lo    0x1008dd088 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x608>
1008dd614:      adrp    x0, 0x101090000 <_anon.68a532d94142320e15103d7866c451bd.21>
1008dd618:      add x0, x0, #0x468
1008dd61c:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008dd620:      adrp    x0, 0x100db1000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x2b20>
1008dd624:      add x0, x0, #0x370
1008dd628:      mov w1, #0xb                ; =11
1008dd62c:      bl  0x100cb1438 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008dd630:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd634:      add x0, x0, #0x790
1008dd638:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008dd63c:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd640:      add x0, x0, #0x888
1008dd644:      bl  0x100c83c6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008dd648:      cmp w8, #0x2
1008dd64c:      b.eq    0x1008dd6b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
1008dd650:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008dd654:      add x1, x1, #0xc0
1008dd658:      mov x22, x0
1008dd65c:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008dd660:      mov x0, x22
1008dd664:      strb    wzr, [x22, #0x20]
1008dd668:      ldr x8, [x22]
1008dd66c:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1008dd670:      cmp x8, x9
1008dd674:      b.lo    0x1008dd1f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x774>
1008dd678:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008dd67c:      add x0, x0, #0xf70
1008dd680:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008dd684:      cmp w8, #0x2
1008dd688:      b.eq    0x1008dd6b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
1008dd68c:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008dd690:      add x1, x1, #0xc0
1008dd694:      mov x19, x0
1008dd698:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008dd69c:      mov x0, x19
1008dd6a0:      strb    wzr, [x19, #0x20]
1008dd6a4:      ldr x8, [x19]
1008dd6a8:      cbz x8, 0x1008dd2e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x868>
1008dd6ac:      b   0x1008dd700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
1008dd6b0:      cmp w8, #0x2
1008dd6b4:      b.ne    0x1008dd6e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc60>
1008dd6b8:      adrp    x0, 0x10108f000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1008dd6bc:      add x0, x0, #0xed8
1008dd6c0:      bl  0x100ccab5c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
1008dd6c4:      adrp    x0, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd6c8:      add x0, x0, #0x700
1008dd6cc:      bl  0x100c83c9c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1008dd6d0:      adrp    x0, 0x100e03000 <_anon.2faa2ae5fa73ebf7e6102d50cd6666c0.1847+0xc23>
1008dd6d4:      add x0, x0, #0x78f
1008dd6d8:      mov w1, #0xb                ; =11
1008dd6dc:      bl  0x100cb1438 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1008dd6e0:      adrp    x1, 0x100952000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtCs5gMwpk3Cs4e_13perry_runtime13async_context20AsyncContextSnapshotEEEB2h_+0x8c>
1008dd6e4:      add x1, x1, #0xc0
1008dd6e8:      mov x19, x0
1008dd6ec:      bl  0x100b91e5c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1008dd6f0:      mov x0, x19
1008dd6f4:      strb    wzr, [x19, #0x20]
1008dd6f8:      ldr x8, [x19]
1008dd6fc:      cbz x8, 0x1008dd59c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb1c>
1008dd700:      adrp    x0, 0x101095000 <_anon.68a532d94142320e15103d7866c451bd.1142>
1008dd704:      add x0, x0, #0x270
1008dd708:      bl  0x100c83c6c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1008dd70c:      mov x22, x24
1008dd710:      adrp    x2, 0x1010c5000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object14class_registry5state20REGISTERED_CLASS_IDS+0x20>
1008dd714:      add x2, x2, #0xff0
1008dd718:      mov x0, x22
1008dd71c:      mov w1, #0x10               ; =16
1008dd720:      bl  0x100c83dcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
