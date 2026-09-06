
/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/escaped-output-worker:   file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010026506c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>:
10026506c:      sub sp, sp, #0x50
100265070:      stp x22, x21, [sp, #0x20]
100265074:      stp x20, x19, [sp, #0x30]
100265078:      stp x29, x30, [sp, #0x40]
10026507c:      add x29, sp, #0x40
100265080:      ldr w8, [x0]
100265084:      cbz w8, 0x100265124 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xb8>
100265088:      cmp w8, #0x1
10026508c:      b.ne    0x1002651a8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x13c>
100265090:      ldr w8, [x0, #0x4]
100265094:      strb    wzr, [sp, #0x4]
100265098:      str wzr, [sp]
10026509c:      and x9, x1, #0xffff000000000000
1002650a0:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1002650a4:      cmp x9, x10
1002650a8:      b.eq    0x1002651d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x164>
1002650ac:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1002650b0:      cmp x9, x10
1002650b4:      b.ne    0x10026532c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2c0>
1002650b8:      ubfx    x9, x1, #40, #8
1002650bc:      cbz x9, 0x10026510c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1002650c0:      strb    w1, [sp]
1002650c4:      cmp x9, #0x1
1002650c8:      b.eq    0x10026510c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1002650cc:      lsr x10, x1, #8
1002650d0:      strb    w10, [sp, #0x1]
1002650d4:      cmp x9, #0x2
1002650d8:      b.eq    0x10026510c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1002650dc:      lsr x10, x1, #16
1002650e0:      strb    w10, [sp, #0x2]
1002650e4:      cmp x9, #0x3
1002650e8:      b.eq    0x10026510c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1002650ec:      lsr x10, x1, #24
1002650f0:      strb    w10, [sp, #0x3]
1002650f4:      cmp x9, #0x4
1002650f8:      b.eq    0x10026510c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0xa0>
1002650fc:      lsr x10, x1, #32
100265100:      strb    w10, [sp, #0x4]
100265104:      cmp x9, #0x5
100265108:      b.ne    0x10026535c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
10026510c:      mov x9, sp
100265110:      mov w10, #0x22              ; =34
100265114:      strb    w10, [x2]
100265118:      mov w11, #0x1               ; =1
10026511c:      cbnz    w8, 0x1002651ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x180>
100265120:      b   0x1002652dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
100265124:      ldr w19, [x0, #0x4]
100265128:      strb    wzr, [sp, #0x4]
10026512c:      str wzr, [sp]
100265130:      and x8, x1, #0xffff000000000000
100265134:      mov x9, #0x7fff000000000000 ; =9223090561878065152
100265138:      cmp x8, x9
10026513c:      b.eq    0x1002652e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x27c>
100265140:      mov x9, #0x7ff9000000000000 ; =9221401712017801216
100265144:      cmp x8, x9
100265148:      b.ne    0x100265344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2d8>
10026514c:      ubfx    x8, x1, #40, #8
100265150:      cbz x8, 0x1002651a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100265154:      strb    w1, [sp]
100265158:      cmp x8, #0x1
10026515c:      b.eq    0x1002651a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100265160:      lsr x9, x1, #8
100265164:      strb    w9, [sp, #0x1]
100265168:      cmp x8, #0x2
10026516c:      b.eq    0x1002651a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100265170:      lsr x9, x1, #16
100265174:      strb    w9, [sp, #0x2]
100265178:      cmp x8, #0x3
10026517c:      b.eq    0x1002651a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100265180:      lsr x9, x1, #24
100265184:      strb    w9, [sp, #0x3]
100265188:      cmp x8, #0x4
10026518c:      b.eq    0x1002651a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x134>
100265190:      lsr x9, x1, #32
100265194:      strb    w9, [sp, #0x4]
100265198:      cmp x8, #0x5
10026519c:      b.ne    0x10026535c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2f0>
1002651a0:      mov x1, sp
1002651a4:      b   0x1002652f4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x288>
1002651a8:      ldur    q0, [x0, #0x8]
1002651ac:      ldur    q1, [x0, #0x18]
1002651b0:      stp q0, q1, [sp]
1002651b4:      ldr w19, [x0, #0x4]
1002651b8:      mov x1, sp
1002651bc:      mov x0, x2
1002651c0:      mov x2, x19
1002651c4:      bl  0x100cd48ec <_writev+0x100cd48ec>
1002651c8:      mov x0, x19
1002651cc:      b   0x100265318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1002651d0:      ands    x9, x1, #0xffffffffffff
1002651d4:      add x9, x9, #0x14
1002651d8:      csel    x9, xzr, x9, eq
1002651dc:      mov w10, #0x22              ; =34
1002651e0:      strb    w10, [x2]
1002651e4:      mov w11, #0x1               ; =1
1002651e8:      cbz w8, 0x1002652dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
1002651ec:      mov w12, #0x5c              ; =92
1002651f0:      mov w13, #0x3075            ; =12405
1002651f4:      mov w14, #0x30              ; =48
1002651f8:      adrp    x15, 0x100db4000 <_anon.06fd7cc7f5a78b698b6304e3c8c6696c.954+0x405>
1002651fc:      add x15, x15, #0xf95
100265200:      b   0x10026521c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1b0>
100265204:      mov w16, #0x62              ; =98
100265208:      strb    w16, [x17, #0x1]
10026520c:      mov w16, #0x2               ; =2
100265210:      add x11, x16, x11
100265214:      subs    x8, x8, #0x1
100265218:      b.eq    0x1002652dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x270>
10026521c:      ldrb    w16, [x9], #0x1
100265220:      cmp w16, #0x20
100265224:      b.lo    0x100265238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
100265228:      cmp w16, #0x5c
10026522c:      b.eq    0x100265238 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1cc>
100265230:      cmp w16, #0x22
100265234:      b.ne    0x1002652c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x254>
100265238:      add x17, x2, x11
10026523c:      strb    w12, [x17]
100265240:      cmp w16, #0xb
100265244:      b.le    0x100265268 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1fc>
100265248:      cmp w16, #0x21
10026524c:      b.gt    0x100265288 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x21c>
100265250:      cmp w16, #0xc
100265254:      b.eq    0x1002652cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x260>
100265258:      cmp w16, #0xd
10026525c:      b.ne    0x100265298 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
100265260:      mov w16, #0x72              ; =114
100265264:      b   0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100265268:      cmp w16, #0x8
10026526c:      b.eq    0x100265204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x198>
100265270:      cmp w16, #0x9
100265274:      b.eq    0x1002652d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x268>
100265278:      cmp w16, #0xa
10026527c:      b.ne    0x100265298 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x22c>
100265280:      mov w16, #0x6e              ; =110
100265284:      b   0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100265288:      cmp w16, #0x22
10026528c:      b.eq    0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100265290:      cmp w16, #0x5c
100265294:      b.eq    0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
100265298:      sturh   w13, [x17, #0x1]
10026529c:      strb    w14, [x17, #0x3]
1002652a0:      lsr x0, x16, #4
1002652a4:      ldrb    w0, [x15, x0]
1002652a8:      strb    w0, [x17, #0x4]
1002652ac:      and x16, x16, #0xf
1002652b0:      ldrb    w16, [x15, x16]
1002652b4:      strb    w16, [x17, #0x5]
1002652b8:      mov w16, #0x6               ; =6
1002652bc:      b   0x100265210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1002652c0:      strb    w16, [x2, x11]
1002652c4:      mov w16, #0x1               ; =1
1002652c8:      b   0x100265210 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x1a4>
1002652cc:      mov w16, #0x66              ; =102
1002652d0:      b   0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1002652d4:      mov w16, #0x74              ; =116
1002652d8:      b   0x100265208 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x19c>
1002652dc:      strb    w10, [x2, x11]
1002652e0:      add x0, x11, #0x1
1002652e4:      b   0x100265318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece+0x2ac>
1002652e8:      ands    x8, x1, #0xffffffffffff
1002652ec:      add x8, x8, #0x14
1002652f0:      csel    x1, xzr, x8, eq
1002652f4:      mov w20, #0x22              ; =34
1002652f8:      mov x0, x2
1002652fc:      strb    w20, [x0], #0x1
100265300:      mov x21, x2
100265304:      mov x2, x19
100265308:      bl  0x100cd48ec <_writev+0x100cd48ec>
10026530c:      add x8, x21, x19
100265310:      strb    w20, [x8, #0x1]
100265314:      add x0, x19, #0x2
100265318:      ldp x29, x30, [sp, #0x40]
10026531c:      ldp x20, x19, [sp, #0x30]
100265320:      ldp x22, x21, [sp, #0x20]
100265324:      add sp, sp, #0x50
100265328:      ret
10026532c:      adrp    x0, 0x100dbc000 <_anon.97abafbc11fe5dd4ea278bc76c84037f.833+0x1a7>
100265330:      add x0, x0, #0x4a6
100265334:      adrp    x2, 0x10109a000 <_anon.97abafbc11fe5dd4ea278bc76c84037f.730+0x60>
100265338:      add x2, x2, #0x1f0
10026533c:      mov w1, #0x20               ; =32
100265340:      bl  0x100c84ac0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
100265344:      adrp    x0, 0x100dbc000 <_anon.97abafbc11fe5dd4ea278bc76c84037f.833+0x1a7>
100265348:      add x0, x0, #0x48e
10026534c:      adrp    x2, 0x10109a000 <_anon.97abafbc11fe5dd4ea278bc76c84037f.730+0x60>
100265350:      add x2, x2, #0x1d8
100265354:      mov w1, #0x18               ; =24
100265358:      bl  0x100c84ac0 <__RNvNtCsjgY6bXVaRmE_4core6option13expect_failed>
10026535c:      adrp    x2, 0x101099000 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object11global_this13fetch_globals22THREAD_MODULE_TOP_THIS+0x1d40>
100265360:      add x2, x2, #0xac8
100265364:      mov w0, #0x5                ; =5
100265368:      mov w1, #0x5                ; =5
10026536c:      bl  0x100c84b8c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
