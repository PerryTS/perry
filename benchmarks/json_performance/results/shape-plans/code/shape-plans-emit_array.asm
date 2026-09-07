/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100918228 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array>:
100918228:      stp x28, x27, [sp, #-0x50]!
10091822c:      stp x24, x23, [sp, #0x10]
100918230:      stp x22, x21, [sp, #0x20]
100918234:      stp x20, x19, [sp, #0x30]
100918238:      stp x29, x30, [sp, #0x40]
10091823c:      add x29, sp, #0x40
100918240:      sub sp, sp, #0x1, lsl #12   ; =0x1000
100918244:      ldr xzr, [sp]
100918248:      sub sp, sp, #0x1, lsl #12   ; =0x1000
10091824c:      ldr xzr, [sp]
100918250:      sub sp, sp, #0x3a0
100918254:      mov x19, x2
100918258:      mov x21, x1
10091825c:      ldr x20, [x2, #0x10]
100918260:      movi.2d v0, #0000000000000000
100918264:      stur    q0, [sp, #0x88]
100918268:      stur    q0, [sp, #0x78]
10091826c:      stur    q0, [sp, #0x68]
100918270:      stur    q0, [sp, #0x58]
100918274:      stur    q0, [sp, #0x48]
100918278:      stur    q0, [sp, #0x38]
10091827c:      stur    q0, [sp, #0x28]
100918280:      stur    q0, [sp, #0x18]
100918284:      str xzr, [sp, #0x2398]
100918288:      mov w8, #0x1                ; =1
10091828c:      stp xzr, x8, [sp]
100918290:      str xzr, [sp, #0x10]
100918294:      ldr x8, [x2]
100918298:      cmp x8, x20
10091829c:      b.eq    0x1009183f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x1cc>
1009182a0:      ldr x8, [x19, #0x8]
1009182a4:      mov w9, #0x5b               ; =91
1009182a8:      strb    w9, [x8, x20]
1009182ac:      add x22, x20, #0x1
1009182b0:      str x22, [x19, #0x10]
1009182b4:      ldr w23, [x0]
1009182b8:      cbz w23, 0x1009182e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xb8>
1009182bc:      ldr x1, [x21]
1009182c0:      mov x0, sp
1009182c4:      mov w2, #0x1                ; =1
1009182c8:      mov x3, x19
1009182cc:      bl  0x1008dc0c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
1009182d0:      cbz w0, 0x1009183a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x17c>
1009182d4:      cmp w23, #0x1
1009182d8:      b.ne    0x100918338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x110>
1009182dc:      ldr x22, [x19, #0x10]
1009182e0:      ldr x8, [x19]
1009182e4:      cmp x8, x22
1009182e8:      b.eq    0x100918418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x1f0>
1009182ec:      ldr x8, [x19, #0x8]
1009182f0:      mov w9, #0x5d               ; =93
1009182f4:      strb    w9, [x8, x22]
1009182f8:      add x20, x22, #0x1
1009182fc:      mov w21, #0x1               ; =1
100918300:      str x20, [x19, #0x10]
100918304:      ldr x8, [sp]
100918308:      cbz x8, 0x100918314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
10091830c:      ldr x0, [sp, #0x8]
100918310:      bl  0x100ce1540 <_mi_free>
100918314:      mov x0, x21
100918318:      add sp, sp, #0x2, lsl #12   ; =0x2000
10091831c:      add sp, sp, #0x3a0
100918320:      ldp x29, x30, [sp, #0x40]
100918324:      ldp x20, x19, [sp, #0x30]
100918328:      ldp x22, x21, [sp, #0x20]
10091832c:      ldp x24, x23, [sp, #0x10]
100918330:      ldp x28, x27, [sp], #0x50
100918334:      ret
100918338:      add x22, x21, #0x8
10091833c:      sub x23, x23, #0x1
100918340:      mov w24, #0x2c              ; =44
100918344:      ldr x21, [x19, #0x10]
100918348:      ldr x8, [x19]
10091834c:      cmp x8, x21
100918350:      b.eq    0x100918388 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x160>
100918354:      ldr x8, [x19, #0x8]
100918358:      strb    w24, [x8, x21]
10091835c:      add x8, x21, #0x1
100918360:      str x8, [x19, #0x10]
100918364:      ldr x1, [x22], #0x8
100918368:      mov x0, sp
10091836c:      mov w2, #0x1                ; =1
100918370:      mov x3, x19
100918374:      bl  0x1008dc0c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter11emit_record>
100918378:      tbz w0, #0x0, 0x1009183a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x17c>
10091837c:      subs    x23, x23, #0x1
100918380:      b.ne    0x100918344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x11c>
100918384:      b   0x1009182dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xb4>
100918388:      mov x0, x19
10091838c:      mov x1, x21
100918390:      mov w2, #0x1                ; =1
100918394:      mov w3, #0x1                ; =1
100918398:      mov w4, #0x1                ; =1
10091839c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1009183a0:      b   0x100918354 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x12c>
1009183a4:      ldr x8, [x19, #0x10]
1009183a8:      cmp x20, x8
1009183ac:      b.ls    0x1009183c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x198>
1009183b0:      mov w21, #0x0               ; =0
1009183b4:      ldr x8, [sp]
1009183b8:      cbnz    x8, 0x10091830c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xe4>
1009183bc:      b   0x100918314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
1009183c0:      mov w21, #0x0               ; =0
1009183c4:      cbz x20, 0x100918300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xd8>
1009183c8:      cmp x20, x8
1009183cc:      b.hs    0x100918300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xd8>
1009183d0:      ldr x8, [x19, #0x8]
1009183d4:      ldrsb   w8, [x8, x20]
1009183d8:      cmn w8, #0x41
1009183dc:      b.le    0x100918434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x20c>
1009183e0:      mov w21, #0x0               ; =0
1009183e4:      str x20, [x19, #0x10]
1009183e8:      ldr x8, [sp]
1009183ec:      cbnz    x8, 0x10091830c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xe4>
1009183f0:      b   0x100918314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xec>
1009183f4:      mov x22, x0
1009183f8:      mov x0, x19
1009183fc:      mov x1, x20
100918400:      mov w2, #0x1                ; =1
100918404:      mov w3, #0x1                ; =1
100918408:      mov w4, #0x1                ; =1
10091840c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100918410:      mov x0, x22
100918414:      b   0x1009182a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0x78>
100918418:      mov x0, x19
10091841c:      mov x1, x22
100918420:      mov w2, #0x1                ; =1
100918424:      mov w3, #0x1                ; =1
100918428:      mov w4, #0x1                ; =1
10091842c:      bl  0x100ccd1c8 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
100918430:      b   0x1009182ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_records10emit_array+0xc4>
100918434:      adrp    x0, 0x100dff000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1976+0x52c>
100918438:      add x0, x0, #0x2b1
10091843c:      adrp    x2, 0x1010d6000 <_anon.ecdcfe4dda90db464027c55ed27f62e6.1732+0x5a68>
100918440:      add x2, x2, #0xf80
100918444:      mov w1, #0x30               ; =48
100918448:      bl  0x100c987e8 <__RNvNtCsjgY6bXVaRmE_4core9panicking5panic>
