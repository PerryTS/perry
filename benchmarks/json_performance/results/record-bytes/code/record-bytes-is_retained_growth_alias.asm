/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010085d5b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias>:
10085d5b4:      stp x24, x23, [sp, #-0x40]!
10085d5b8:      stp x22, x21, [sp, #0x10]
10085d5bc:      stp x20, x19, [sp, #0x20]
10085d5c0:      stp x29, x30, [sp, #0x30]
10085d5c4:      add x29, sp, #0x30
10085d5c8:      mov x19, x1
10085d5cc:      lsr x8, x0, #48
10085d5d0:      mov w9, #0x7ffd             ; =32765
10085d5d4:      and x10, x0, #0xffffffffffff
10085d5d8:      cmp w8, #0x0
10085d5dc:      csel    x11, x8, x0, ne
10085d5e0:      cset    w12, ne
10085d5e4:      cmp x8, x9
10085d5e8:      csel    x20, x10, x11, eq
10085d5ec:      csel    w8, wzr, w12, eq
10085d5f0:      lsr x10, x1, #48
10085d5f4:      cbz x10, 0x10085d604 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x50>
10085d5f8:      cmp w10, w9
10085d5fc:      b.ne    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d600:      and x19, x19, #0xffffffffffff
10085d604:      cmp x20, x19
10085d608:      csinc   w8, w8, wzr, ne
10085d60c:      tbz w8, #0x0, 0x10085d628 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x74>
10085d610:      mov w0, #0x0                ; =0
10085d614:      ldp x29, x30, [sp, #0x30]
10085d618:      ldp x20, x19, [sp, #0x20]
10085d61c:      ldp x22, x21, [sp, #0x10]
10085d620:      ldp x24, x23, [sp], #0x40
10085d624:      ret
10085d628:      mov w8, #0x3f               ; =63
10085d62c:      adrp    x22, 0x101134000 <_perry_global_baseline_worker_ts__1>
10085d630:      add x22, x22, #0x8f0
10085d634:      mov x23, x8
10085d638:      mov x0, x20
10085d63c:      bl  0x1008610b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10085d640:      mov x21, x0
10085d644:      mov w0, #0x0                ; =0
10085d648:      cbz x20, 0x10085d614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
10085d64c:      cbz x21, 0x10085d614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
10085d650:      ldr x8, [x22]
10085d654:      cmn x8, #0x1
10085d658:      b.eq    0x10085d794 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
10085d65c:      mrs x9, TPIDRRO_EL0
10085d660:      and x9, x9, #0xfffffffffffffff8
10085d664:      ldr x0, [x9, x8, lsl #3]
10085d668:      cbz x0, 0x10085d794 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1e0>
10085d66c:      lsr x1, x20, #20
10085d670:      ldr x8, [x0, #0x10]
10085d674:      ldrb    w9, [x8, #0x28]
10085d678:      tbz w9, #0x0, 0x10085d698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
10085d67c:      ldr x9, [x8, #0x20]
10085d680:      cmp x9, x1
10085d684:      b.ne    0x10085d698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
10085d688:      ldp x9, x10, [x8]
10085d68c:      cmp x20, x9
10085d690:      ccmp    x20, x10, #0x2, hs
10085d694:      b.lo    0x10085d704 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x150>
10085d698:      ldrb    w9, [x8, #0x58]
10085d69c:      cbz w9, 0x10085d6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
10085d6a0:      ldr x9, [x8, #0x50]
10085d6a4:      cmp x9, x1
10085d6a8:      b.ne    0x10085d6bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x108>
10085d6ac:      ldp x9, x10, [x8, #0x30]
10085d6b0:      cmp x20, x9
10085d6b4:      ccmp    x20, x10, #0x2, hs
10085d6b8:      b.lo    0x10085d76c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1b8>
10085d6bc:      ldrb    w9, [x8, #0x88]
10085d6c0:      cbz w9, 0x10085d6e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
10085d6c4:      ldr x9, [x8, #0x80]
10085d6c8:      cmp x9, x1
10085d6cc:      b.ne    0x10085d6e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x12c>
10085d6d0:      ldp x9, x10, [x8, #0x60]
10085d6d4:      cmp x20, x9
10085d6d8:      ccmp    x20, x10, #0x2, hs
10085d6dc:      b.lo    0x10085d780 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1cc>
10085d6e0:      ldrb    w9, [x8, #0xb8]
10085d6e4:      cbz w9, 0x10085d710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
10085d6e8:      ldr x9, [x8, #0xb0]
10085d6ec:      cmp x9, x1
10085d6f0:      b.ne    0x10085d710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
10085d6f4:      ldp x9, x10, [x8, #0x90]!
10085d6f8:      cmp x20, x9
10085d6fc:      ccmp    x20, x10, #0x2, hs
10085d700:      b.hs    0x10085d710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
10085d704:      ldrb    w8, [x8, #0x19]
10085d708:      cmp w8, #0xff
10085d70c:      b.ne    0x10085d71c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
10085d710:      mov x0, x20
10085d714:      bl  0x10045cfb0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
10085d718:      and w8, w0, #0xff
10085d71c:      and w8, w8, #0xfe
10085d720:      cmp w8, #0x2
10085d724:      b.ne    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d728:      ldrb    w8, [x21]
10085d72c:      cmp w8, #0x1
10085d730:      b.ne    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d734:      ldrsb   w8, [x21, #0x1]
10085d738:      tbz w8, #0x1f, 0x10085d7ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x1f8>
10085d73c:      ldrh    w8, [x21, #0x2]
10085d740:      lsr w8, w8, #14
10085d744:      cmp w8, #0x2
10085d748:      b.ls    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d74c:      mov w0, #0x0                ; =0
10085d750:      ldr x9, [x21, #0x8]
10085d754:      cmp x20, x9
10085d758:      b.eq    0x10085d614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
10085d75c:      sub w8, w23, #0x1
10085d760:      mov x20, x9
10085d764:      cbnz    w23, 0x10085d634 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x80>
10085d768:      b   0x10085d614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
10085d76c:      add x8, x8, #0x30
10085d770:      ldrb    w8, [x8, #0x19]
10085d774:      cmp w8, #0xff
10085d778:      b.ne    0x10085d71c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
10085d77c:      b   0x10085d710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
10085d780:      add x8, x8, #0x60
10085d784:      ldrb    w8, [x8, #0x19]
10085d788:      cmp w8, #0xff
10085d78c:      b.ne    0x10085d71c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x168>
10085d790:      b   0x10085d710 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x15c>
10085d794:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
10085d798:      lsr x1, x20, #20
10085d79c:      ldr x8, [x0, #0x10]
10085d7a0:      ldrb    w9, [x8, #0x28]
10085d7a4:      tbnz    w9, #0x0, 0x10085d67c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xc8>
10085d7a8:      b   0x10085d698 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0xe4>
10085d7ac:      cmp x20, x19
10085d7b0:      b.ne    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d7b4:      ldrh    w8, [x21, #0x2]
10085d7b8:      lsr w8, w8, #14
10085d7bc:      cmp w8, #0x2
10085d7c0:      b.hi    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d7c4:      ldp w9, w8, [x20]
10085d7c8:      cmp w9, w8
10085d7cc:      b.hi    0x10085d610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x5c>
10085d7d0:      ldr w9, [x21, #0x4]
10085d7d4:      subs    x9, x9, #0x10
10085d7d8:      csel    x9, xzr, x9, lo
10085d7dc:      cmp x8, x9, lsr #3
10085d7e0:      cset    w0, ls
10085d7e4:      b   0x10085d614 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array17growth_forwarding24is_retained_growth_alias+0x60>
