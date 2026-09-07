/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/key33-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c3640 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record>:
1004c3640:      stp x28, x27, [sp, #-0x60]!
1004c3644:      stp x26, x25, [sp, #0x10]
1004c3648:      stp x24, x23, [sp, #0x20]
1004c364c:      stp x22, x21, [sp, #0x30]
1004c3650:      stp x20, x19, [sp, #0x40]
1004c3654:      stp x29, x30, [sp, #0x50]
1004c3658:      add x29, sp, #0x50
1004c365c:      sub sp, sp, #0x500
1004c3660:      ldr xzr, [sp]
1004c3664:      mov x22, x0
1004c3668:      mov x8, #0x200000002        ; =8589934594
1004c366c:      stp x1, x8, [sp, #0x30]
1004c3670:      str wzr, [sp, #0x40]
1004c3674:      str x8, [sp, #0x48]
1004c3678:      str wzr, [sp, #0x50]
1004c367c:      str x8, [sp, #0x58]
1004c3680:      str wzr, [sp, #0x60]
1004c3684:      str x8, [sp, #0x68]
1004c3688:      str wzr, [sp, #0x70]
1004c368c:      str x8, [sp, #0x78]
1004c3690:      str wzr, [sp, #0x80]
1004c3694:      str x8, [sp, #0x88]
1004c3698:      str wzr, [sp, #0x90]
1004c369c:      str x8, [sp, #0x98]
1004c36a0:      str wzr, [sp, #0xa0]
1004c36a4:      str x8, [sp, #0xa8]
1004c36a8:      str wzr, [sp, #0xb0]
1004c36ac:      movi.2d v0, #0000000000000000
1004c36b0:      str d0, [sp, #0xb8]
1004c36b4:      str wzr, [sp, #0xc0]
1004c36b8:      str d0, [sp, #0xe0]
1004c36bc:      str wzr, [sp, #0xe8]
1004c36c0:      str d0, [sp, #0x108]
1004c36c4:      str wzr, [sp, #0x110]
1004c36c8:      str d0, [sp, #0x130]
1004c36cc:      str wzr, [sp, #0x138]
1004c36d0:      str d0, [sp, #0x158]
1004c36d4:      str wzr, [sp, #0x160]
1004c36d8:      str d0, [sp, #0x180]
1004c36dc:      str wzr, [sp, #0x188]
1004c36e0:      str d0, [sp, #0x1a8]
1004c36e4:      str wzr, [sp, #0x1b0]
1004c36e8:      str d0, [sp, #0x1d0]
1004c36ec:      str wzr, [sp, #0x1d8]
1004c36f0:      str d0, [sp, #0x1f8]
1004c36f4:      str wzr, [sp, #0x200]
1004c36f8:      str d0, [sp, #0x220]
1004c36fc:      str wzr, [sp, #0x228]
1004c3700:      str d0, [sp, #0x248]
1004c3704:      str wzr, [sp, #0x250]
1004c3708:      str d0, [sp, #0x270]
1004c370c:      str wzr, [sp, #0x278]
1004c3710:      str d0, [sp, #0x298]
1004c3714:      str wzr, [sp, #0x2a0]
1004c3718:      str d0, [sp, #0x2c0]
1004c371c:      str wzr, [sp, #0x2c8]
1004c3720:      str d0, [sp, #0x2e8]
1004c3724:      str wzr, [sp, #0x2f0]
1004c3728:      str d0, [sp, #0x310]
1004c372c:      str wzr, [sp, #0x318]
1004c3730:      str d0, [sp, #0x338]
1004c3734:      str wzr, [sp, #0x340]
1004c3738:      str d0, [sp, #0x360]
1004c373c:      str wzr, [sp, #0x368]
1004c3740:      str d0, [sp, #0x388]
1004c3744:      str wzr, [sp, #0x390]
1004c3748:      str d0, [sp, #0x3b0]
1004c374c:      str wzr, [sp, #0x3b8]
1004c3750:      str d0, [sp, #0x3d8]
1004c3754:      str wzr, [sp, #0x3e0]
1004c3758:      str d0, [sp, #0x400]
1004c375c:      str wzr, [sp, #0x408]
1004c3760:      str d0, [sp, #0x428]
1004c3764:      str wzr, [sp, #0x430]
1004c3768:      str d0, [sp, #0x450]
1004c376c:      mov w8, #-0x40000000        ; =-1073741824
1004c3770:      ldr w20, [x0, #0x4]
1004c3774:      cmp w20, w8
1004c3778:      csel    w19, w20, w8, lt
1004c377c:      str wzr, [sp, #0x458]
1004c3780:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3784:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3788:      ldr w21, [x9, #0x634]
1004c378c:      cmp w21, #0x300
1004c3790:      b.hs    0x1004c3824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c3794:      ldr x8, [x8, #0x48]
1004c3798:      cmn x8, #0x1
1004c379c:      b.eq    0x1004c3814 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c37a0:      mrs x9, TPIDRRO_EL0
1004c37a4:      and x9, x9, #0xfffffffffffffff8
1004c37a8:      ldr x0, [x9, x8, lsl #3]
1004c37ac:      cbz x0, 0x1004c3814 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1d4>
1004c37b0:      add x8, x0, x21, lsl #3
1004c37b4:      ldr x0, [x8, #0x1e8]
1004c37b8:      cbz x0, 0x1004c3824 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
1004c37bc:      ldr x0, [x0]
1004c37c0:      cbz x0, 0x1004c3838 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x1f8>
1004c37c4:      mov w8, #-0x40000001        ; =-1073741825
1004c37c8:      cmp w20, w8
1004c37cc:      b.gt    0x1004c3848 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c37d0:      ldr x10, [x0, #0x5198]
1004c37d4:      and w8, w19, #0x3fffffff
1004c37d8:      lsr x9, x8, #15
1004c37dc:      cmp x9, x10
1004c37e0:      b.hs    0x1004c3848 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c37e4:      ldr x10, [x0, #0x5190]
1004c37e8:      ldr x9, [x10, x9, lsl #3]
1004c37ec:      cbz x9, 0x1004c3848 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c37f0:      ubfx    x10, x8, #5, #10
1004c37f4:      ldr x9, [x9, x10, lsl #3]
1004c37f8:      cbz x9, 0x1004c3848 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c37fc:      and x8, x8, #0x1f
1004c3800:      add x8, x9, x8, lsl #5
1004c3804:      ldrb    w9, [x8, #0x1c]
1004c3808:      tbz w9, #0x0, 0x1004c3848 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x208>
1004c380c:      ldr x8, [x8]
1004c3810:      b   0x1004c384c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x20c>
1004c3814:      bl  0x100adbcb0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3818:      add x8, x0, x21, lsl #3
1004c381c:      ldr x0, [x8, #0x1e8]
1004c3820:      cbnz    x0, 0x1004c37bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x17c>
1004c3824:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1c8>
1004c3828:      add x0, x0, #0x348
1004c382c:      bl  0x100ad911c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3830:      ldr x0, [x0]
1004c3834:      cbnz    x0, 0x1004c37c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x184>
1004c3838:      bl  0x100adadec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c383c:      mov w8, #-0x40000001        ; =-1073741825
1004c3840:      cmp w20, w8
1004c3844:      b.le    0x1004c37d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x190>
1004c3848:      mov x8, #0x0                ; =0
1004c384c:      mov x28, #0x0               ; =0
1004c3850:      mov x25, #0x0               ; =0
1004c3854:      add x9, x8, #0x8
1004c3858:      add x8, x22, #0x10
1004c385c:      stp x8, x9, [sp, #0x20]
1004c3860:      add x8, sp, #0x1f8
1004c3864:      add x8, x8, #0x8
1004c3868:      stp x22, x8, [sp, #0x10]
1004c386c:      mov w22, #0x2               ; =2
1004c3870:      mov w20, #0x28              ; =40
1004c3874:      mov w27, #0x2               ; =2
1004c3878:      ldr x8, [sp, #0x28]
1004c387c:      ldr x0, [x8, x28, lsl #3]
1004c3880:      bl  0x10021ad34 <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan3new>
1004c3884:      cbz x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3888:      mov x21, x0
1004c388c:      add x8, sp, #0x38
1004c3890:      add x8, x8, x28, lsl #4
1004c3894:      str x0, [x8]
1004c3898:      str w1, [x8, #0x8]
1004c389c:      ldr x8, [sp, #0x20]
1004c38a0:      ldr x23, [x8, x28, lsl #3]
1004c38a4:      sub x0, x29, #0xd8
1004c38a8:      mov x1, x23
1004c38ac:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c38b0:      ldur    w8, [x29, #-0xd8]
1004c38b4:      cmn w8, #0x1
1004c38b8:      b.eq    0x1004c3910 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x2d0>
1004c38bc:      ldp w9, w10, [x29, #-0xd4]
1004c38c0:      ldur    w11, [x29, #-0xcc]
1004c38c4:      add x12, sp, #0xb8
1004c38c8:      madd    x12, x28, x20, x12
1004c38cc:      stp w8, w9, [x12]
1004c38d0:      stp w10, w11, [x12, #0x8]
1004c38d4:      sub x13, x29, #0xd8
1004c38d8:      ldur    q0, [x13, #0x10]
1004c38dc:      str q0, [x12, #0x10]
1004c38e0:      ldur    x13, [x13, #0x20]
1004c38e4:      str x13, [x12, #0x20]
1004c38e8:      cbz w8, 0x1004c3aac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x46c>
1004c38ec:      cmp w8, #0x1
1004c38f0:      csel    w23, w10, w9, eq
1004c38f4:      csel    w20, w11, w9, eq
1004c38f8:      cmp x28, #0x0
1004c38fc:      mov w8, #0x1                ; =1
1004c3900:      cinc    w8, w8, ne
1004c3904:      adds    w9, w22, w21
1004c3908:      b.lo    0x1004c3aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4ac>
1004c390c:      b   0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3910:      mov w8, #0x7ffd             ; =32765
1004c3914:      cmp x8, x23, lsr #48
1004c3918:      b.ne    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c391c:      and x23, x23, #0xffffffffffff
1004c3920:      mov x0, x23
1004c3924:      bl  0x100535bc4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004c3928:      cbz x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c392c:      ldrb    w8, [x0]
1004c3930:      cmp w8, #0x1
1004c3934:      b.ne    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3938:      ldrsb   w8, [x0, #0x1]
1004c393c:      tbnz    w8, #0x1f, 0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3940:      ldrh    w8, [x0, #0x2]
1004c3944:      tbnz    w8, #0xa, 0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3948:      ldr w8, [x0, #0x4]
1004c394c:      cmp w8, #0x10
1004c3950:      b.lo    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3954:      ldr w19, [x23]
1004c3958:      cmp w19, #0x10
1004c395c:      b.hi    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3960:      ldr w9, [x23, #0x4]
1004c3964:      cmp w19, w9
1004c3968:      b.hi    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c396c:      lsl x24, x19, #3
1004c3970:      add x9, x24, #0x10
1004c3974:      cmp x9, x8
1004c3978:      b.hi    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c397c:      mov x0, x23
1004c3980:      bl  0x1004fb46c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array6header35array_has_named_properties_resolved>
1004c3984:      tbnz    w0, #0x0, 0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3988:      mov x0, x23
1004c398c:      bl  0x1005672c8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c3990:      cmp x0, #0x1
1004c3994:      b.eq    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3998:      add x10, x25, x19
1004c399c:      cmp x10, #0x10
1004c39a0:      b.hi    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c39a4:      add x8, sp, #0xb8
1004c39a8:      madd    x8, x28, x20, x8
1004c39ac:      mov w9, #-0x1               ; =-1
1004c39b0:      str w9, [x8]
1004c39b4:      stp x25, x19, [x8, #0x8]
1004c39b8:      cbz w19, 0x1004c3acc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x48c>
1004c39bc:      str x10, [sp, #0x8]
1004c39c0:      mov x26, #0x0               ; =0
1004c39c4:      add x19, x23, #0x8
1004c39c8:      ldr x8, [sp, #0x18]
1004c39cc:      madd    x25, x25, x20, x8
1004c39d0:      mov w20, #0x2               ; =2
1004c39d4:      mov w23, #0x2               ; =2
1004c39d8:      ldr x1, [x19, x26]
1004c39dc:      sub x0, x29, #0x90
1004c39e0:      bl  0x1004bdbc0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c39e4:      ldur    w8, [x29, #-0x90]
1004c39e8:      cmn w8, #0x1
1004c39ec:      b.eq    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c39f0:      ldp w10, w9, [x29, #-0x8c]
1004c39f4:      ldur    w11, [x29, #-0x84]
1004c39f8:      sub x13, x29, #0x90
1004c39fc:      ldur    q0, [x13, #0x10]
1004c3a00:      sub x12, x29, #0xb0
1004c3a04:      str q0, [x12]
1004c3a08:      ldur    x12, [x13, #0x20]
1004c3a0c:      stur    x12, [x29, #-0xa0]
1004c3a10:      stp w8, w10, [x25, #-0x8]
1004c3a14:      stp w9, w11, [x25]
1004c3a18:      stur    q0, [x25, #0x8]
1004c3a1c:      str x12, [x25, #0x18]
1004c3a20:      cbz w8, 0x1004c3a44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x404>
1004c3a24:      cmp w8, #0x1
1004c3a28:      csel    w8, w11, w10, eq
1004c3a2c:      csel    w10, w9, w10, eq
1004c3a30:      cmp x26, #0x0
1004c3a34:      cset    w9, ne
1004c3a38:      adds    w10, w10, w23
1004c3a3c:      b.lo    0x1004c3a5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x41c>
1004c3a40:      b   0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3a44:      add w10, w10, #0x2
1004c3a48:      add w8, w9, #0x2
1004c3a4c:      cmp x26, #0x0
1004c3a50:      cset    w9, ne
1004c3a54:      adds    w10, w10, w23
1004c3a58:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3a5c:      adds    w23, w10, w9
1004c3a60:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3a64:      mov x0, #0x0                ; =0
1004c3a68:      adds    w8, w8, w20
1004c3a6c:      cset    w10, hs
1004c3a70:      adds    w20, w8, w9
1004c3a74:      cset    w8, hs
1004c3a78:      tbnz    w10, #0x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3a7c:      tbnz    w8, #0x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3a80:      add x26, x26, #0x8
1004c3a84:      add x25, x25, #0x28
1004c3a88:      cmp x24, x26
1004c3a8c:      b.ne    0x1004c39d8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x398>
1004c3a90:      ldr x25, [sp, #0x8]
1004c3a94:      cmp x28, #0x0
1004c3a98:      mov w8, #0x1                ; =1
1004c3a9c:      cinc    w8, w8, ne
1004c3aa0:      adds    w9, w22, w21
1004c3aa4:      b.lo    0x1004c3aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4ac>
1004c3aa8:      b   0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3aac:      add w23, w9, #0x2
1004c3ab0:      add w20, w10, #0x2
1004c3ab4:      cmp x28, #0x0
1004c3ab8:      mov w8, #0x1                ; =1
1004c3abc:      cinc    w8, w8, ne
1004c3ac0:      adds    w9, w22, w21
1004c3ac4:      b.lo    0x1004c3aec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4ac>
1004c3ac8:      b   0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3acc:      mov w23, #0x2               ; =2
1004c3ad0:      mov w20, #0x2               ; =2
1004c3ad4:      mov x25, x10
1004c3ad8:      cmp x28, #0x0
1004c3adc:      mov w8, #0x1                ; =1
1004c3ae0:      cinc    w8, w8, ne
1004c3ae4:      adds    w9, w22, w21
1004c3ae8:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3aec:      adds    w9, w23, w9
1004c3af0:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3af4:      adds    w22, w9, w8
1004c3af8:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3afc:      lsr x9, x21, #32
1004c3b00:      adds    w9, w27, w9
1004c3b04:      b.hs    0x1004c3c10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d0>
1004c3b08:      mov x0, #0x0                ; =0
1004c3b0c:      adds    w9, w20, w9
1004c3b10:      cset    w10, hs
1004c3b14:      adds    w27, w9, w8
1004c3b18:      cset    w8, hs
1004c3b1c:      tbnz    w10, #0x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3b20:      tbnz    w8, #0x0, 0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3b24:      add x28, x28, #0x1
1004c3b28:      ldr x8, [sp, #0x30]
1004c3b2c:      cmp x28, x8
1004c3b30:      mov w20, #0x28              ; =40
1004c3b34:      b.ne    0x1004c3878 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x238>
1004c3b38:      bl  0x10026abd0 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c3b3c:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1004c3b40:      mov w8, #0x1                ; =1
1004c3b44:      ldr x19, [sp, #0x10]
1004c3b48:      stp x19, x9, [x29, #-0x88]
1004c3b4c:      stp x0, x8, [x29, #-0x98]
1004c3b50:      sub x0, x29, #0x90
1004c3b54:      bl  0x10026acd8 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c3b58:      mov x23, x0
1004c3b5c:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c3b60:      add x0, x0, #0xbf0
1004c3b64:      ldr x8, [x0]
1004c3b68:      blr x8
1004c3b6c:      strb    wzr, [x0]
1004c3b70:      mov x0, x19
1004c3b74:      bl  0x1004c3280 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
1004c3b78:      tbz w0, #0x0, 0x1004c3c08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5c8>
1004c3b7c:      mov x0, x22
1004c3b80:      bl  0x100325f8c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004c3b84:      mov x25, x0
1004c3b88:      mov x21, x1
1004c3b8c:      stp w27, w22, [x0]
1004c3b90:      stp wzr, wzr, [x0, #0xc]
1004c3b94:      str w22, [x0, #0x8]
1004c3b98:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3b9c:      ldr x8, [x24, #0x48]
1004c3ba0:      cmn x8, #0x1
1004c3ba4:      b.eq    0x1004c3c34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5f4>
1004c3ba8:      mrs x9, TPIDRRO_EL0
1004c3bac:      and x9, x9, #0xfffffffffffffff8
1004c3bb0:      ldr x8, [x9, x8, lsl #3]
1004c3bb4:      cbz x8, 0x1004c3c34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5f4>
1004c3bb8:      ldr x8, [x8, #0x19e8]
1004c3bbc:      cbz x8, 0x1004c3c34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5f4>
1004c3bc0:      ldr x9, [x8]
1004c3bc4:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c3bc8:      cmp x9, x10
1004c3bcc:      b.hs    0x1004c3f00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
1004c3bd0:      add x10, x9, #0x1
1004c3bd4:      str x10, [x8]
1004c3bd8:      ldr x10, [x8, #0x18]
1004c3bdc:      cmp x23, x10
1004c3be0:      b.hs    0x1004c3f0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8cc>
1004c3be4:      ldr x10, [x8, #0x10]
1004c3be8:      mov w11, #0x18              ; =24
1004c3bec:      madd    x10, x23, x11, x10
1004c3bf0:      ldr x11, [x10]
1004c3bf4:      cmp x11, #0x1
1004c3bf8:      b.ne    0x1004c3f10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8d0>
1004c3bfc:      ldr x0, [x10, #0x8]
1004c3c00:      str x9, [x8]
1004c3c04:      b   0x1004c3c3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5fc>
1004c3c08:      sub x0, x29, #0x98
1004c3c0c:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3c10:      mov x0, #0x0                ; =0
1004c3c14:      add sp, sp, #0x500
1004c3c18:      ldp x29, x30, [sp, #0x50]
1004c3c1c:      ldp x20, x19, [sp, #0x40]
1004c3c20:      ldp x22, x21, [sp, #0x30]
1004c3c24:      ldp x24, x23, [sp, #0x20]
1004c3c28:      ldp x26, x25, [sp, #0x10]
1004c3c2c:      ldp x28, x27, [sp], #0x60
1004c3c30:      ret
1004c3c34:      mov x0, x23
1004c3c38:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c3c3c:      ldr w20, [x0, #0x4]
1004c3c40:      mov w8, #-0x40000000        ; =-1073741824
1004c3c44:      cmp w20, w8
1004c3c48:      csel    w19, w20, w8, lt
1004c3c4c:      adrp    x8, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3c50:      ldr w22, [x8, #0x634]
1004c3c54:      cmp w22, #0x300
1004c3c58:      b.hs    0x1004c3cfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6bc>
1004c3c5c:      ldr x8, [x24, #0x48]
1004c3c60:      cmn x8, #0x1
1004c3c64:      b.eq    0x1004c3ce0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6a0>
1004c3c68:      mrs x9, TPIDRRO_EL0
1004c3c6c:      and x9, x9, #0xfffffffffffffff8
1004c3c70:      ldr x8, [x9, x8, lsl #3]
1004c3c74:      cbz x8, 0x1004c3ce0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6a0>
1004c3c78:      add x8, x8, x22, lsl #3
1004c3c7c:      ldr x8, [x8, #0x1e8]
1004c3c80:      cbz x8, 0x1004c3cfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6bc>
1004c3c84:      ldr x8, [x8]
1004c3c88:      cbz x8, 0x1004c3d20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6e0>
1004c3c8c:      mov w9, #-0x40000001        ; =-1073741825
1004c3c90:      cmp w20, w9
1004c3c94:      str x25, [sp, #0x18]
1004c3c98:      b.gt    0x1004c3d40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x700>
1004c3c9c:      ldr x11, [x8, #0x5198]
1004c3ca0:      and w9, w19, #0x3fffffff
1004c3ca4:      lsr x10, x9, #15
1004c3ca8:      cmp x10, x11
1004c3cac:      b.hs    0x1004c3d40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x700>
1004c3cb0:      ldr x8, [x8, #0x5190]
1004c3cb4:      ldr x8, [x8, x10, lsl #3]
1004c3cb8:      cbz x8, 0x1004c3d44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3cbc:      ubfx    x10, x9, #5, #10
1004c3cc0:      ldr x8, [x8, x10, lsl #3]
1004c3cc4:      cbz x8, 0x1004c3d44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3cc8:      and x9, x9, #0x1f
1004c3ccc:      add x8, x8, x9, lsl #5
1004c3cd0:      ldrb    w9, [x8, #0x1c]
1004c3cd4:      tbz w9, #0x0, 0x1004c3d40 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x700>
1004c3cd8:      ldr x8, [x8]
1004c3cdc:      b   0x1004c3d44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3ce0:      mov x23, x0
1004c3ce4:      bl  0x100adbcb0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3ce8:      mov x8, x0
1004c3cec:      mov x0, x23
1004c3cf0:      add x8, x8, x22, lsl #3
1004c3cf4:      ldr x8, [x8, #0x1e8]
1004c3cf8:      cbnz    x8, 0x1004c3c84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x644>
1004c3cfc:      adrp    x8, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1c8>
1004c3d00:      add x8, x8, #0x348
1004c3d04:      mov x22, x0
1004c3d08:      mov x0, x8
1004c3d0c:      bl  0x100ad911c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3d10:      mov x8, x0
1004c3d14:      mov x0, x22
1004c3d18:      ldr x8, [x8]
1004c3d1c:      cbnz    x8, 0x1004c3c8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x64c>
1004c3d20:      mov x22, x0
1004c3d24:      bl  0x100adadec <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c3d28:      mov x8, x0
1004c3d2c:      mov x0, x22
1004c3d30:      mov w9, #-0x40000001        ; =-1073741825
1004c3d34:      cmp w20, w9
1004c3d38:      str x25, [sp, #0x18]
1004c3d3c:      b.le    0x1004c3c9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3d40:      mov x8, #0x0                ; =0
1004c3d44:      mov x19, #0x0               ; =0
1004c3d48:      mov w9, #0x7b               ; =123
1004c3d4c:      strb    w9, [x21]
1004c3d50:      add x20, x8, #0x8
1004c3d54:      add x25, x0, #0x10
1004c3d58:      add x8, sp, #0x1f8
1004c3d5c:      add x8, x8, #0x28
1004c3d60:      stp x8, x20, [sp, #0x20]
1004c3d64:      mov w22, #0x1               ; =1
1004c3d68:      add x23, sp, #0x38
1004c3d6c:      mov w24, #0x3a              ; =58
1004c3d70:      mov w26, #0x28              ; =40
1004c3d74:      add x27, sp, #0xb8
1004c3d78:      mov w28, #0x2c              ; =44
1004c3d7c:      b   0x1004c3dac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x76c>
1004c3d80:      mov w8, #0x5d               ; =93
1004c3d84:      strb    w8, [x21, x24]
1004c3d88:      add x22, x24, #0x1
1004c3d8c:      ldp x20, x8, [sp, #0x28]
1004c3d90:      add x23, sp, #0x38
1004c3d94:      mov w24, #0x3a              ; =58
1004c3d98:      mov w26, #0x28              ; =40
1004c3d9c:      add x27, sp, #0xb8
1004c3da0:      add x19, x19, #0x1
1004c3da4:      cmp x19, x8
1004c3da8:      b.eq    0x1004c3ed8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x898>
1004c3dac:      cbz x19, 0x1004c3db8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x778>
1004c3db0:      strb    w28, [x21, x22]
1004c3db4:      add x22, x22, #0x1
1004c3db8:      add x8, x23, x19, lsl #4
1004c3dbc:      ldr x0, [x8]
1004c3dc0:      ldr w1, [x8, #0x8]
1004c3dc4:      ldr x2, [x20, x19, lsl #3]
1004c3dc8:      add x3, x21, x22
1004c3dcc:      bl  0x10021b34c <__RNvMNtNtCs7wa3bwJW6LN_13perry_runtime4json18stringify_key_planNtB2_4Plan5write>
1004c3dd0:      add x8, x0, x22
1004c3dd4:      strb    w24, [x21, x8]
1004c3dd8:      add x22, x8, #0x1
1004c3ddc:      ldr x1, [x25, x19, lsl #3]
1004c3de0:      madd    x0, x19, x26, x27
1004c3de4:      ldr w9, [x0]
1004c3de8:      cmn w9, #0x1
1004c3dec:      b.eq    0x1004c3e10 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7d0>
1004c3df0:      add x2, x21, x22
1004c3df4:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3df8:      add x22, x0, x22
1004c3dfc:      add x19, x19, #0x1
1004c3e00:      ldr x8, [sp, #0x30]
1004c3e04:      cmp x19, x8
1004c3e08:      b.ne    0x1004c3dac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x76c>
1004c3e0c:      b   0x1004c3ed8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x898>
1004c3e10:      ldp x23, x20, [x0, #0x8]
1004c3e14:      add x24, x8, #0x2
1004c3e18:      mov w8, #0x5b               ; =91
1004c3e1c:      strb    w8, [x21, x22]
1004c3e20:      cbz x20, 0x1004c3d80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x740>
1004c3e24:      cmp x23, #0xf
1004c3e28:      b.hi    0x1004c3f20 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
1004c3e2c:      and x22, x1, #0xffffffffffff
1004c3e30:      add x8, sp, #0x1f8
1004c3e34:      madd    x8, x23, x26, x8
1004c3e38:      ldp q0, q1, [x8]
1004c3e3c:      sub x9, x29, #0xb0
1004c3e40:      stp q0, q1, [x9, #0x20]
1004c3e44:      ldr x8, [x8, #0x20]
1004c3e48:      stur    x8, [x29, #-0x70]
1004c3e4c:      ldr x1, [x22, #0x8]
1004c3e50:      sub x0, x29, #0x90
1004c3e54:      add x2, x21, x24
1004c3e58:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3e5c:      add x24, x0, x24
1004c3e60:      subs    x27, x20, #0x1
1004c3e64:      mov w9, #0x28               ; =40
1004c3e68:      b.eq    0x1004c3d80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x740>
1004c3e6c:      add x26, x22, #0x10
1004c3e70:      cmp x23, #0x10
1004c3e74:      mov w8, #0x10               ; =16
1004c3e78:      csel    x8, x23, x8, lo
1004c3e7c:      sub x20, x8, #0xf
1004c3e80:      add x22, x23, #0x1
1004c3e84:      ldr x8, [sp, #0x20]
1004c3e88:      madd    x23, x23, x9, x8
1004c3e8c:      strb    w28, [x21, x24]
1004c3e90:      cbz x20, 0x1004c3f24 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e4>
1004c3e94:      add x24, x24, #0x1
1004c3e98:      ldp q0, q1, [x23]
1004c3e9c:      sub x8, x29, #0xb0
1004c3ea0:      stp q0, q1, [x8, #0x20]
1004c3ea4:      ldr x8, [x23, #0x20]
1004c3ea8:      stur    x8, [x29, #-0x70]
1004c3eac:      ldr x1, [x26], #0x8
1004c3eb0:      sub x0, x29, #0x90
1004c3eb4:      add x2, x21, x24
1004c3eb8:      bl  0x1004bd128 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3ebc:      add x24, x0, x24
1004c3ec0:      add x20, x20, #0x1
1004c3ec4:      add x22, x22, #0x1
1004c3ec8:      add x23, x23, #0x28
1004c3ecc:      subs    x27, x27, #0x1
1004c3ed0:      b.ne    0x1004c3e8c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x84c>
1004c3ed4:      b   0x1004c3d80 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x740>
1004c3ed8:      mov w8, #0x7d               ; =125
1004c3edc:      strb    w8, [x21, x22]
1004c3ee0:      mov x19, #0x7fff000000000000 ; =9223090561878065152
1004c3ee4:      ldr x8, [sp, #0x18]
1004c3ee8:      bfxil   x19, x8, #0, #48
1004c3eec:      sub x0, x29, #0x98
1004c3ef0:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3ef4:      mov x1, x19
1004c3ef8:      mov w0, #0x1                ; =1
1004c3efc:      b   0x1004c3c14 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x5d4>
1004c3f00:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c3f04:      add x0, x0, #0x8a8
1004c3f08:      bl  0x100ab12dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c3f0c:      bl  0x100ae2b88 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c3f10:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbf0>
1004c3f14:      add x0, x0, #0x6c
1004c3f18:      mov w1, #0xb                ; =11
1004c3f1c:      bl  0x100ae2b50 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c3f20:      mov x22, x23
1004c3f24:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ed8>
1004c3f28:      add x2, x2, #0x18
1004c3f2c:      mov x0, x22
1004c3f30:      mov w1, #0x10               ; =16
1004c3f34:      bl  0x100ab140c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
