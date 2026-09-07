/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/record-bytes-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001008b54a0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan>:
1008b54a0:      sub sp, sp, #0x120
1008b54a4:      stp x28, x27, [sp, #0xc0]
1008b54a8:      stp x26, x25, [sp, #0xd0]
1008b54ac:      stp x24, x23, [sp, #0xe0]
1008b54b0:      stp x22, x21, [sp, #0xf0]
1008b54b4:      stp x20, x19, [sp, #0x100]
1008b54b8:      stp x29, x30, [sp, #0x110]
1008b54bc:      add x29, sp, #0x110
1008b54c0:      mov x19, x0
1008b54c4:      adrp    x8, 0x101135000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime14typed_feedback8REGISTRY+0x28>
1008b54c8:      add x8, x8, #0xfcc
1008b54cc:      ldr w20, [x8]
1008b54d0:      cmp w20, #0x300
1008b54d4:      b.hs    0x1008b55f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x154>
1008b54d8:      adrp    x8, 0x101134000 <_perry_global_baseline_worker_ts__1>
1008b54dc:      add x8, x8, #0x8f0
1008b54e0:      ldr x8, [x8]
1008b54e4:      cmn x8, #0x1
1008b54e8:      b.eq    0x1008b55cc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x12c>
1008b54ec:      mrs x9, TPIDRRO_EL0
1008b54f0:      and x9, x9, #0xfffffffffffffff8
1008b54f4:      ldr x0, [x9, x8, lsl #3]
1008b54f8:      cbz x0, 0x1008b55cc <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x12c>
1008b54fc:      add x8, x0, x20, lsl #3
1008b5500:      ldr x0, [x8, #0x1e8]
1008b5504:      cbz x0, 0x1008b55f4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x154>
1008b5508:      ldr x0, [x0]
1008b550c:      cbz x0, 0x1008b5620 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x180>
1008b5510:      mov w8, #-0x40000001        ; =-1073741825
1008b5514:      cmp w2, w8
1008b5518:      b.gt    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b551c:      ldr x10, [x0, #0x5198]
1008b5520:      and w8, w2, #0x3fffffff
1008b5524:      lsr x9, x8, #15
1008b5528:      cmp x9, x10
1008b552c:      b.hs    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5530:      ldr x10, [x0, #0x5190]
1008b5534:      ldr x9, [x10, x9, lsl #3]
1008b5538:      cbz x9, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b553c:      ubfx    x10, x8, #5, #10
1008b5540:      ldr x9, [x9, x10, lsl #3]
1008b5544:      cbz x9, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5548:      and x8, x8, #0x1f
1008b554c:      add x8, x9, x8, lsl #5
1008b5550:      ldrb    w9, [x8, #0x1c]
1008b5554:      tbz w9, #0x0, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5558:      ldr x20, [x8]
1008b555c:      ldr w21, [x8, #0x14]
1008b5560:      cbz x20, 0x1008b564c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ac>
1008b5564:      mov x23, x1
1008b5568:      mov x24, x2
1008b556c:      mov x25, x3
1008b5570:      mov x0, x20
1008b5574:      bl  0x100902d58 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1008b5578:      cbz x0, 0x1008b5690 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008b557c:      ldrb    w8, [x0]
1008b5580:      cmp w8, #0x1
1008b5584:      b.ne    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5588:      ldrsb   w8, [x0, #0x1]
1008b558c:      tbnz    w8, #0x1f, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5590:      ldr w8, [x0, #0x4]
1008b5594:      cmp w8, #0x10
1008b5598:      b.lo    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b559c:      ldp w22, w9, [x20]
1008b55a0:      cmp w22, #0x20
1008b55a4:      ccmp    w22, w9, #0x2, ls
1008b55a8:      b.hi    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b55ac:      lsl x9, x22, #3
1008b55b0:      add x9, x9, #0x10
1008b55b4:      cmp x9, x8
1008b55b8:      mov x3, x25
1008b55bc:      mov x2, x24
1008b55c0:      mov x1, x23
1008b55c4:      b.ls    0x1008b5650 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1b0>
1008b55c8:      b   0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b55cc:      mov x21, x3
1008b55d0:      mov x22, x2
1008b55d4:      mov x23, x1
1008b55d8:      bl  0x100cd2ac8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1008b55dc:      mov x1, x23
1008b55e0:      mov x2, x22
1008b55e4:      mov x3, x21
1008b55e8:      add x8, x0, x20, lsl #3
1008b55ec:      ldr x0, [x8, #0x1e8]
1008b55f0:      cbnz    x0, 0x1008b5508 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x68>
1008b55f4:      adrp    x0, 0x1010e5000 <_anon.15617d145d7456405150372c8522e2e6.886+0x20>
1008b55f8:      add x0, x0, #0x278
1008b55fc:      mov x20, x3
1008b5600:      mov x22, x2
1008b5604:      mov x21, x1
1008b5608:      bl  0x100cd2598 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1008b560c:      mov x1, x21
1008b5610:      mov x2, x22
1008b5614:      mov x3, x20
1008b5618:      ldr x0, [x0]
1008b561c:      cbnz    x0, 0x1008b5510 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x70>
1008b5620:      mov x20, x3
1008b5624:      mov x21, x2
1008b5628:      mov x22, x1
1008b562c:      bl  0x100cd4db8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1008b5630:      mov x1, x22
1008b5634:      mov x2, x21
1008b5638:      mov x3, x20
1008b563c:      mov w8, #-0x40000001        ; =-1073741825
1008b5640:      cmp w2, w8
1008b5644:      b.le    0x1008b551c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x7c>
1008b5648:      b   0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b564c:      mov x22, #0x0               ; =0
1008b5650:      mov x0, #0x0                ; =0
1008b5654:      cmp x22, x21
1008b5658:      b.hi    0x1008b5690 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008b565c:      mov x8, x1
1008b5660:      ldr x9, [x19, #0x2398]
1008b5664:      cmp x9, #0x40
1008b5668:      b.eq    0x1008b5690 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008b566c:      mov x21, x2
1008b5670:      mov x25, x3
1008b5674:      mov x0, x8
1008b5678:      bl  0x10068258c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe36to_json_definitely_absent_without_gc>
1008b567c:      cbz w0, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5680:      mov x0, x20
1008b5684:      bl  0x1006c0b8c <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime6object13field_get_set11enumeration24keys_contain_array_index>
1008b5688:      tbz w0, #0x0, 0x1008b56b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x210>
1008b568c:      mov x0, #0x0                ; =0
1008b5690:      ldp x29, x30, [sp, #0x110]
1008b5694:      ldp x20, x19, [sp, #0x100]
1008b5698:      ldp x22, x21, [sp, #0xf0]
1008b569c:      ldp x24, x23, [sp, #0xe0]
1008b56a0:      ldp x26, x25, [sp, #0xd0]
1008b56a4:      ldp x28, x27, [sp, #0xc0]
1008b56a8:      add sp, sp, #0x120
1008b56ac:      ret
1008b56b0:      add x24, sp, #0x2c
1008b56b4:      movi.2d v0, #0000000000000000
1008b56b8:      stur    q0, [x24, #0x7c]
1008b56bc:      stur    q0, [x24, #0x6c]
1008b56c0:      stur    q0, [x24, #0x5c]
1008b56c4:      stur    q0, [sp, #0x78]
1008b56c8:      stur    q0, [sp, #0x68]
1008b56cc:      stur    q0, [sp, #0x58]
1008b56d0:      stur    q0, [sp, #0x48]
1008b56d4:      stur    q0, [sp, #0x38]
1008b56d8:      stp w21, w22, [sp, #0x2c]
1008b56dc:      mov x9, x19
1008b56e0:      ldr x8, [x19, #0x10]
1008b56e4:      str w8, [sp, #0x34]
1008b56e8:      strb    wzr, [sp, #0xc]
1008b56ec:      str wzr, [sp, #0x8]
1008b56f0:      cbz x22, 0x1008b585c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x3bc>
1008b56f4:      ldr x9, [x20, #0x8]
1008b56f8:      and x10, x9, #0xffff000000000000
1008b56fc:      mov x11, #0x7fff000000000000 ; =9223090561878065152
1008b5700:      cmp x10, x11
1008b5704:      b.eq    0x1008b5734 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x294>
1008b5708:      mov x11, #0x7ff9000000000000 ; =9221401712017801216
1008b570c:      cmp x10, x11
1008b5710:      b.ne    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5714:      ubfx    x10, x9, #40, #8
1008b5718:      cbz x10, 0x1008b5748 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2a8>
1008b571c:      strb    w9, [sp, #0x8]
1008b5720:      cmp x10, #0x1
1008b5724:      b.ne    0x1008b5754 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2b4>
1008b5728:      add x0, sp, #0x8
1008b572c:      mov w1, #0x1                ; =1
1008b5730:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b5734:      ands    x9, x9, #0xffffffffffff
1008b5738:      b.eq    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b573c:      ldr w1, [x9, #0x4]
1008b5740:      add x0, x9, #0x14
1008b5744:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b5748:      mov x1, #0x0                ; =0
1008b574c:      add x0, sp, #0x8
1008b5750:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b5754:      lsr x11, x9, #8
1008b5758:      strb    w11, [sp, #0x9]
1008b575c:      cmp x10, #0x2
1008b5760:      b.ne    0x1008b5770 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2d0>
1008b5764:      add x0, sp, #0x8
1008b5768:      mov w1, #0x2                ; =2
1008b576c:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b5770:      lsr x11, x9, #16
1008b5774:      strb    w11, [sp, #0xa]
1008b5778:      cmp x10, #0x3
1008b577c:      b.ne    0x1008b578c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x2ec>
1008b5780:      add x0, sp, #0x8
1008b5784:      mov w1, #0x3                ; =3
1008b5788:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b578c:      lsr x11, x9, #24
1008b5790:      strb    w11, [sp, #0xb]
1008b5794:      cmp x10, #0x4
1008b5798:      b.ne    0x1008b57a8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x308>
1008b579c:      add x0, sp, #0x8
1008b57a0:      mov w1, #0x4                ; =4
1008b57a4:      b   0x1008b57c0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x320>
1008b57a8:      lsr x9, x9, #32
1008b57ac:      strb    w9, [sp, #0xc]
1008b57b0:      cmp x10, #0x5
1008b57b4:      b.ne    0x1008b5af4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x654>
1008b57b8:      add x0, sp, #0x8
1008b57bc:      mov w1, #0x5                ; =5
1008b57c0:      cmp x8, #0x10, lsl #12      ; =0x10000
1008b57c4:      b.hi    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b57c8:      mov w9, #0x6                ; =6
1008b57cc:      umull   x9, w1, w9
1008b57d0:      mov w10, #0x10000           ; =65536
1008b57d4:      add x9, x9, #0x4
1008b57d8:      sub x8, x10, x8
1008b57dc:      cmp x9, x8
1008b57e0:      b.hi    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b57e4:      add x8, sp, #0x10
1008b57e8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008b57ec:      ldr w8, [sp, #0x10]
1008b57f0:      tbnz    w8, #0x0, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b57f4:      ldp x1, x2, [sp, #0x18]
1008b57f8:      mov x8, x19
1008b57fc:      ldr x21, [x19, #0x10]
1008b5800:      ldr x9, [x19]
1008b5804:      cmp x9, x21
1008b5808:      b.eq    0x1008b5a90 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5f0>
1008b580c:      ldr x9, [x8, #0x8]
1008b5810:      mov w10, #0x7b              ; =123
1008b5814:      strb    w10, [x9, x21]
1008b5818:      add x9, x21, #0x1
1008b581c:      str x9, [x8, #0x10]
1008b5820:      mov x0, x19
1008b5824:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008b5828:      mov x9, x19
1008b582c:      ldr x21, [x19, #0x10]
1008b5830:      ldr x8, [x19]
1008b5834:      cmp x8, x21
1008b5838:      b.eq    0x1008b5ac0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x620>
1008b583c:      ldr x8, [x9, #0x8]
1008b5840:      mov w10, #0x3a              ; =58
1008b5844:      strb    w10, [x8, x21]
1008b5848:      add x8, x21, #0x1
1008b584c:      str x8, [x9, #0x10]
1008b5850:      str w8, [sp, #0x38]
1008b5854:      subs    x28, x22, #0x1
1008b5858:      b.ne    0x1008b58d8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x438>
1008b585c:      ldr x1, [x9, #0x2398]
1008b5860:      cmp x1, #0x40
1008b5864:      b.hs    0x1008b5ae0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x640>
1008b5868:      mov w8, #0x8c               ; =140
1008b586c:      madd    x8, x1, x8, x9
1008b5870:      ldur    q0, [sp, #0x2c]
1008b5874:      ldur    q1, [sp, #0x3c]
1008b5878:      stur    q0, [x8, #0x98]
1008b587c:      stur    q1, [x8, #0xa8]
1008b5880:      ldur    q0, [sp, #0x4c]
1008b5884:      ldur    q1, [sp, #0x5c]
1008b5888:      stur    q0, [x8, #0xb8]
1008b588c:      stur    q1, [x8, #0xc8]
1008b5890:      ldp q0, q1, [x24, #0x60]
1008b5894:      stur    q0, [x8, #0xf8]
1008b5898:      ldur    q0, [sp, #0x7c]
1008b589c:      ldur    q2, [sp, #0x6c]
1008b58a0:      stur    q0, [x8, #0xe8]
1008b58a4:      stur    q2, [x8, #0xd8]
1008b58a8:      add x8, x8, #0x98
1008b58ac:      str q1, [x8, #0x70]
1008b58b0:      ldur    q0, [x24, #0x7c]
1008b58b4:      stur    q0, [x8, #0x7c]
1008b58b8:      ldr x8, [x9, #0x2398]
1008b58bc:      add x8, x8, #0x1
1008b58c0:      str x8, [x9, #0x2398]
1008b58c4:      add x8, x9, x25
1008b58c8:      add w9, w1, #0x1
1008b58cc:      strb    w9, [x8, #0x18]
1008b58d0:      mov w0, #0x1                ; =1
1008b58d4:      b   0x1008b5690 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1f0>
1008b58d8:      add x27, x20, #0x10
1008b58dc:      add x9, sp, #0x2c
1008b58e0:      add x26, x9, #0x10
1008b58e4:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1008b58e8:      ldr x9, [x27], #0x8
1008b58ec:      and x11, x9, #0xffff000000000000
1008b58f0:      cmp x11, x10
1008b58f4:      b.eq    0x1008b5918 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x478>
1008b58f8:      mov x10, #0x7fff000000000000 ; =9223090561878065152
1008b58fc:      cmp x11, x10
1008b5900:      b.ne    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b5904:      ands    x9, x9, #0xffffffffffff
1008b5908:      b.eq    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b590c:      ldr w1, [x9, #0x4]
1008b5910:      add x0, x9, #0x14
1008b5914:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b5918:      ubfx    x10, x9, #40, #8
1008b591c:      cbz x10, 0x1008b5938 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x498>
1008b5920:      strb    w9, [sp, #0x8]
1008b5924:      cmp x10, #0x1
1008b5928:      b.ne    0x1008b5944 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4a4>
1008b592c:      add x0, sp, #0x8
1008b5930:      mov w1, #0x1                ; =1
1008b5934:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b5938:      mov x1, #0x0                ; =0
1008b593c:      add x0, sp, #0x8
1008b5940:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b5944:      lsr x11, x9, #8
1008b5948:      strb    w11, [sp, #0x9]
1008b594c:      cmp x10, #0x2
1008b5950:      b.ne    0x1008b5960 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4c0>
1008b5954:      add x0, sp, #0x8
1008b5958:      mov w1, #0x2                ; =2
1008b595c:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b5960:      lsr x11, x9, #16
1008b5964:      strb    w11, [sp, #0xa]
1008b5968:      cmp x10, #0x3
1008b596c:      b.ne    0x1008b597c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4dc>
1008b5970:      add x0, sp, #0x8
1008b5974:      mov w1, #0x3                ; =3
1008b5978:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b597c:      lsr x11, x9, #24
1008b5980:      strb    w11, [sp, #0xb]
1008b5984:      cmp x10, #0x4
1008b5988:      b.ne    0x1008b5998 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x4f8>
1008b598c:      add x0, sp, #0x8
1008b5990:      mov w1, #0x4                ; =4
1008b5994:      b   0x1008b59b0 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x510>
1008b5998:      lsr x9, x9, #32
1008b599c:      strb    w9, [sp, #0xc]
1008b59a0:      cmp x10, #0x5
1008b59a4:      b.ne    0x1008b5af4 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x654>
1008b59a8:      add x0, sp, #0x8
1008b59ac:      mov w1, #0x5                ; =5
1008b59b0:      cmp x8, #0x10, lsl #12      ; =0x10000
1008b59b4:      b.hi    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b59b8:      mov w9, #0x6                ; =6
1008b59bc:      umull   x9, w1, w9
1008b59c0:      add x9, x9, #0x4
1008b59c4:      mov w10, #0x10000           ; =65536
1008b59c8:      sub x8, x10, x8
1008b59cc:      cmp x9, x8
1008b59d0:      b.hi    0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b59d4:      add x8, sp, #0x10
1008b59d8:      bl  0x10002db98 <__RNvNtNtCsjgY6bXVaRmE_4core3str8converts9from_utf8>
1008b59dc:      ldr x8, [sp, #0x10]
1008b59e0:      cbnz    x8, 0x1008b568c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x1ec>
1008b59e4:      ldp x20, x21, [sp, #0x18]
1008b59e8:      ldr x22, [x19, #0x10]
1008b59ec:      ldr x8, [x19]
1008b59f0:      cmp x8, x22
1008b59f4:      b.eq    0x1008b5a58 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5b8>
1008b59f8:      ldr x8, [x19, #0x8]
1008b59fc:      mov w9, #0x2c               ; =44
1008b5a00:      strb    w9, [x8, x22]
1008b5a04:      add x8, x22, #0x1
1008b5a08:      str x8, [x19, #0x10]
1008b5a0c:      mov x0, x19
1008b5a10:      mov x1, x20
1008b5a14:      mov x2, x21
1008b5a18:      bl  0x1008edb14 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json17stringify_scalars20write_escaped_string>
1008b5a1c:      ldr x20, [x19, #0x10]
1008b5a20:      ldr x8, [x19]
1008b5a24:      cmp x8, x20
1008b5a28:      b.eq    0x1008b5a74 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x5d4>
1008b5a2c:      mov x9, x19
1008b5a30:      ldr x8, [x19, #0x8]
1008b5a34:      mov w10, #0x3a              ; =58
1008b5a38:      strb    w10, [x8, x20]
1008b5a3c:      add x8, x20, #0x1
1008b5a40:      str x8, [x19, #0x10]
1008b5a44:      str w8, [x26], #0x4
1008b5a48:      subs    x28, x28, #0x1
1008b5a4c:      mov x10, #0x7ff9000000000000 ; =9221401712017801216
1008b5a50:      b.ne    0x1008b58e8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x448>
1008b5a54:      b   0x1008b585c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x3bc>
1008b5a58:      mov x0, x19
1008b5a5c:      mov x1, x22
1008b5a60:      mov w2, #0x1                ; =1
1008b5a64:      mov w3, #0x1                ; =1
1008b5a68:      mov w4, #0x1                ; =1
1008b5a6c:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5a70:      b   0x1008b59f8 <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x558>
1008b5a74:      mov x0, x19
1008b5a78:      mov x1, x20
1008b5a7c:      mov w2, #0x1                ; =1
1008b5a80:      mov w3, #0x1                ; =1
1008b5a84:      mov w4, #0x1                ; =1
1008b5a88:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5a8c:      b   0x1008b5a2c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x58c>
1008b5a90:      mov x0, x19
1008b5a94:      mov x23, x1
1008b5a98:      mov x1, x21
1008b5a9c:      mov x26, x2
1008b5aa0:      mov w2, #0x1                ; =1
1008b5aa4:      mov w3, #0x1                ; =1
1008b5aa8:      mov w4, #0x1                ; =1
1008b5aac:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5ab0:      mov x2, x26
1008b5ab4:      mov x1, x23
1008b5ab8:      mov x8, x19
1008b5abc:      b   0x1008b580c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x36c>
1008b5ac0:      mov x0, x19
1008b5ac4:      mov x1, x21
1008b5ac8:      mov w2, #0x1                ; =1
1008b5acc:      mov w3, #0x1                ; =1
1008b5ad0:      mov w4, #0x1                ; =1
1008b5ad4:      bl  0x100cd4074 <__RINvNvMs2_NtCsctvjasLqLe9_5alloc7raw_vecINtB8_11RawVecInnerpE7reserve21do_reserve_and_handleNtNtBa_5alloc6GlobalECs5gMwpk3Cs4e_13perry_runtime>
1008b5ad8:      mov x9, x19
1008b5adc:      b   0x1008b583c <__RNvMNtNtCs5gMwpk3Cs4e_13perry_runtime4json24stringify_nested_recordsNtB2_7Emitter8add_plan+0x39c>
1008b5ae0:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008b5ae4:      add x2, x2, #0xcc8
1008b5ae8:      mov x0, x1
1008b5aec:      mov w1, #0x40               ; =64
1008b5af0:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
1008b5af4:      adrp    x2, 0x1010dc000 <_anon.17c5d9a448d3eabdc7a96a2547784904.1186+0x64e8>
1008b5af8:      add x2, x2, #0xcf8
1008b5afc:      mov w0, #0x5                ; =5
1008b5b00:      mov w1, #0x5                ; =5
1008b5b04:      bl  0x100c9dfcc <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
