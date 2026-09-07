/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/growth-alias-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001005912b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>:
1005912b8:      stp x24, x23, [sp, #-0x40]!
1005912bc:      stp x22, x21, [sp, #0x10]
1005912c0:      stp x20, x19, [sp, #0x20]
1005912c4:      stp x29, x30, [sp, #0x30]
1005912c8:      add x29, sp, #0x30
1005912cc:      mov x19, x1
1005912d0:      lsr x8, x0, #48
1005912d4:      mov w9, #0x7ffd             ; =32765
1005912d8:      and x10, x0, #0xffffffffffff
1005912dc:      cmp w8, #0x0
1005912e0:      csel    x11, x8, x0, ne
1005912e4:      cset    w12, ne
1005912e8:      cmp x8, x9
1005912ec:      csel    x20, x10, x11, eq
1005912f0:      csel    w8, wzr, w12, eq
1005912f4:      lsr x10, x1, #48
1005912f8:      cbz x10, 0x100591308 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x50>
1005912fc:      cmp w10, w9
100591300:      b.ne    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100591304:      and x19, x19, #0xffffffffffff
100591308:      cmp x20, x19
10059130c:      csinc   w8, w8, wzr, ne
100591310:      tbz w8, #0x0, 0x10059132c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x74>
100591314:      mov w0, #0x0                ; =0
100591318:      ldp x29, x30, [sp, #0x30]
10059131c:      ldp x20, x19, [sp, #0x20]
100591320:      ldp x22, x21, [sp, #0x10]
100591324:      ldp x24, x23, [sp], #0x40
100591328:      ret
10059132c:      mov w8, #0x3f               ; =63
100591330:      adrp    x22, 0x101121000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x30>
100591334:      add x22, x22, #0xfe0
100591338:      mov x23, x8
10059133c:      mov x0, x20
100591340:      bl  0x1005ac688 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100591344:      mov x21, x0
100591348:      mov w0, #0x0                ; =0
10059134c:      cbz x20, 0x100591318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100591350:      cbz x21, 0x100591318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100591354:      ldr x8, [x22]
100591358:      cmn x8, #0x1
10059135c:      b.eq    0x100591498 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
100591360:      mrs x9, TPIDRRO_EL0
100591364:      and x9, x9, #0xfffffffffffffff8
100591368:      ldr x0, [x9, x8, lsl #3]
10059136c:      cbz x0, 0x100591498 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
100591370:      lsr x1, x20, #20
100591374:      ldr x8, [x0, #0x10]
100591378:      ldrb    w9, [x8, #0x28]
10059137c:      tbz w9, #0x0, 0x10059139c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
100591380:      ldr x9, [x8, #0x20]
100591384:      cmp x9, x1
100591388:      b.ne    0x10059139c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
10059138c:      ldp x9, x10, [x8]
100591390:      cmp x20, x9
100591394:      ccmp    x20, x10, #0x2, hs
100591398:      b.lo    0x100591408 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x150>
10059139c:      ldrb    w9, [x8, #0x58]
1005913a0:      cbz w9, 0x1005913c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
1005913a4:      ldr x9, [x8, #0x50]
1005913a8:      cmp x9, x1
1005913ac:      b.ne    0x1005913c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
1005913b0:      ldp x9, x10, [x8, #0x30]
1005913b4:      cmp x20, x9
1005913b8:      ccmp    x20, x10, #0x2, hs
1005913bc:      b.lo    0x100591470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1b8>
1005913c0:      ldrb    w9, [x8, #0x88]
1005913c4:      cbz w9, 0x1005913e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
1005913c8:      ldr x9, [x8, #0x80]
1005913cc:      cmp x9, x1
1005913d0:      b.ne    0x1005913e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
1005913d4:      ldp x9, x10, [x8, #0x60]
1005913d8:      cmp x20, x9
1005913dc:      ccmp    x20, x10, #0x2, hs
1005913e0:      b.lo    0x100591484 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1cc>
1005913e4:      ldrb    w9, [x8, #0xb8]
1005913e8:      cbz w9, 0x100591414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1005913ec:      ldr x9, [x8, #0xb0]
1005913f0:      cmp x9, x1
1005913f4:      b.ne    0x100591414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
1005913f8:      ldp x9, x10, [x8, #0x90]!
1005913fc:      cmp x20, x9
100591400:      ccmp    x20, x10, #0x2, hs
100591404:      b.hs    0x100591414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100591408:      ldrb    w8, [x8, #0x19]
10059140c:      cmp w8, #0xff
100591410:      b.ne    0x100591420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
100591414:      mov x0, x20
100591418:      bl  0x1009960b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
10059141c:      and w8, w0, #0xff
100591420:      and w8, w8, #0xfe
100591424:      cmp w8, #0x2
100591428:      b.ne    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10059142c:      ldrb    w8, [x21]
100591430:      cmp w8, #0x1
100591434:      b.ne    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100591438:      ldrsb   w8, [x21, #0x1]
10059143c:      tbz w8, #0x1f, 0x1005914b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1f8>
100591440:      ldrh    w8, [x21, #0x2]
100591444:      lsr w8, w8, #14
100591448:      cmp w8, #0x2
10059144c:      b.ls    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
100591450:      mov w0, #0x0                ; =0
100591454:      ldr x9, [x21, #0x8]
100591458:      cmp x20, x9
10059145c:      b.eq    0x100591318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100591460:      sub w8, w23, #0x1
100591464:      mov x20, x9
100591468:      cbnz    w23, 0x100591338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x80>
10059146c:      b   0x100591318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
100591470:      add x8, x8, #0x30
100591474:      ldrb    w8, [x8, #0x19]
100591478:      cmp w8, #0xff
10059147c:      b.ne    0x100591420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
100591480:      b   0x100591414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100591484:      add x8, x8, #0x60
100591488:      ldrb    w8, [x8, #0x19]
10059148c:      cmp w8, #0xff
100591490:      b.ne    0x100591420 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
100591494:      b   0x100591414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
100591498:      bl  0x100cb1624 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10059149c:      lsr x1, x20, #20
1005914a0:      ldr x8, [x0, #0x10]
1005914a4:      ldrb    w9, [x8, #0x28]
1005914a8:      tbnz    w9, #0x0, 0x100591380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xc8>
1005914ac:      b   0x10059139c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
1005914b0:      cmp x20, x19
1005914b4:      b.ne    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005914b8:      ldrh    w8, [x21, #0x2]
1005914bc:      lsr w8, w8, #14
1005914c0:      cmp w8, #0x2
1005914c4:      b.hi    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005914c8:      ldp w9, w8, [x20]
1005914cc:      cmp w9, w8
1005914d0:      b.hi    0x100591314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
1005914d4:      ldr w9, [x21, #0x4]
1005914d8:      subs    x9, x9, #0x10
1005914dc:      csel    x9, xzr, x9, lo
1005914e0:      cmp x8, x9, lsr #3
1005914e4:      cset    w0, ls
1005914e8:      b   0x100591318 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
        ...
