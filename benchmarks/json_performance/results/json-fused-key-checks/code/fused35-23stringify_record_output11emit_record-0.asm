/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/fused35-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001004c3640 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record>:
1004c3640:      stp x28, x27, [sp, #-0x60]!
1004c3644:      stp x26, x25, [sp, #0x10]
1004c3648:      stp x24, x23, [sp, #0x20]
1004c364c:      stp x22, x21, [sp, #0x30]
1004c3650:      stp x20, x19, [sp, #0x40]
1004c3654:      stp x29, x30, [sp, #0x50]
1004c3658:      add x29, sp, #0x50
1004c365c:      sub sp, sp, #0x5d0
1004c3660:      ldr xzr, [sp]
1004c3664:      str x1, [sp, #0x38]
1004c3668:      mov x22, x0
1004c366c:      movi.2d v0, #0000000000000000
1004c3670:      str d0, [sp, #0x40]
1004c3674:      str wzr, [sp, #0x48]
1004c3678:      str d0, [sp, #0x68]
1004c367c:      str wzr, [sp, #0x70]
1004c3680:      str d0, [sp, #0x90]
1004c3684:      str wzr, [sp, #0x98]
1004c3688:      str d0, [sp, #0xb8]
1004c368c:      str wzr, [sp, #0xc0]
1004c3690:      str d0, [sp, #0xe0]
1004c3694:      str wzr, [sp, #0xe8]
1004c3698:      str d0, [sp, #0x108]
1004c369c:      str wzr, [sp, #0x110]
1004c36a0:      str d0, [sp, #0x130]
1004c36a4:      str wzr, [sp, #0x138]
1004c36a8:      str d0, [sp, #0x158]
1004c36ac:      str wzr, [sp, #0x160]
1004c36b0:      str d0, [sp, #0x180]
1004c36b4:      str wzr, [sp, #0x188]
1004c36b8:      str d0, [sp, #0x1a8]
1004c36bc:      str wzr, [sp, #0x1b0]
1004c36c0:      str d0, [sp, #0x1d0]
1004c36c4:      str wzr, [sp, #0x1d8]
1004c36c8:      str d0, [sp, #0x1f8]
1004c36cc:      str wzr, [sp, #0x200]
1004c36d0:      str d0, [sp, #0x220]
1004c36d4:      str wzr, [sp, #0x228]
1004c36d8:      str d0, [sp, #0x248]
1004c36dc:      str wzr, [sp, #0x250]
1004c36e0:      str d0, [sp, #0x270]
1004c36e4:      str wzr, [sp, #0x278]
1004c36e8:      str d0, [sp, #0x298]
1004c36ec:      str wzr, [sp, #0x2a0]
1004c36f0:      str d0, [sp, #0x2c0]
1004c36f4:      str wzr, [sp, #0x2c8]
1004c36f8:      str d0, [sp, #0x2e8]
1004c36fc:      str wzr, [sp, #0x2f0]
1004c3700:      str d0, [sp, #0x310]
1004c3704:      str wzr, [sp, #0x318]
1004c3708:      str d0, [sp, #0x338]
1004c370c:      str wzr, [sp, #0x340]
1004c3710:      str d0, [sp, #0x360]
1004c3714:      str wzr, [sp, #0x368]
1004c3718:      str d0, [sp, #0x388]
1004c371c:      str wzr, [sp, #0x390]
1004c3720:      str d0, [sp, #0x3b0]
1004c3724:      str wzr, [sp, #0x3b8]
1004c3728:      str d0, [sp, #0x3d8]
1004c372c:      str wzr, [sp, #0x3e0]
1004c3730:      str d0, [sp, #0x400]
1004c3734:      str wzr, [sp, #0x408]
1004c3738:      str d0, [sp, #0x428]
1004c373c:      str wzr, [sp, #0x430]
1004c3740:      str d0, [sp, #0x450]
1004c3744:      str wzr, [sp, #0x458]
1004c3748:      str d0, [sp, #0x478]
1004c374c:      str wzr, [sp, #0x480]
1004c3750:      str d0, [sp, #0x4a0]
1004c3754:      str wzr, [sp, #0x4a8]
1004c3758:      str d0, [sp, #0x4c8]
1004c375c:      str wzr, [sp, #0x4d0]
1004c3760:      str d0, [sp, #0x4f0]
1004c3764:      str wzr, [sp, #0x4f8]
1004c3768:      str d0, [sp, #0x518]
1004c376c:      mov w8, #-0x40000000        ; =-1073741824
1004c3770:      ldr w20, [x0, #0x4]
1004c3774:      cmp w20, w8
1004c3778:      csel    w19, w20, w8, lt
1004c377c:      str wzr, [sp, #0x520]
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
1004c3814:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3818:      add x8, x0, x21, lsl #3
1004c381c:      ldr x0, [x8, #0x1e8]
1004c3820:      cbnz    x0, 0x1004c37bc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x17c>
1004c3824:      adrp    x0, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004c3828:      add x0, x0, #0x330
1004c382c:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3830:      ldr x0, [x0]
1004c3834:      cbnz    x0, 0x1004c37c4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x184>
1004c3838:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c383c:      mov w8, #-0x40000001        ; =-1073741825
1004c3840:      cmp w20, w8
1004c3844:      b.le    0x1004c37d0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x190>
1004c3848:      mov x8, #0x0                ; =0
1004c384c:      mov x23, #0x0               ; =0
1004c3850:      mov x19, #0x0               ; =0
1004c3854:      add x9, x8, #0x8
1004c3858:      sub x28, x29, #0x80
1004c385c:      add x8, x22, #0x10
1004c3860:      stp x8, x9, [sp, #0x28]
1004c3864:      add x8, sp, #0x2c0
1004c3868:      add x8, x8, #0x8
1004c386c:      stp x22, x8, [sp, #0x8]
1004c3870:      mov w22, #0x2               ; =2
1004c3874:      mov w25, #0x28              ; =40
1004c3878:      mov w26, #0x2               ; =2
1004c387c:      ldr x8, [sp, #0x30]
1004c3880:      ldr x1, [x8, x23, lsl #3]
1004c3884:      sub x0, x29, #0x80
1004c3888:      bl  0x1004bdd30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat9key_piece>
1004c388c:      ldur    w8, [x29, #-0x80]
1004c3890:      cmn w8, #0x1
1004c3894:      b.eq    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3898:      ldp w10, w9, [x29, #-0x7c]
1004c389c:      ldur    w11, [x29, #-0x74]
1004c38a0:      ldur    q0, [x28, #0x10]
1004c38a4:      stur    q0, [x29, #-0xe0]
1004c38a8:      ldur    x12, [x28, #0x20]
1004c38ac:      stur    x12, [x29, #-0xd0]
1004c38b0:      add x13, sp, #0x40
1004c38b4:      madd    x13, x23, x25, x13
1004c38b8:      stp w8, w10, [x13]
1004c38bc:      stp w9, w11, [x13, #0x8]
1004c38c0:      str q0, [x13, #0x10]
1004c38c4:      str x12, [x13, #0x20]
1004c38c8:      cbz w8, 0x1004c38dc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x29c>
1004c38cc:      cmp w8, #0x1
1004c38d0:      csel    w24, w11, w10, eq
1004c38d4:      csel    w27, w9, w10, eq
1004c38d8:      b   0x1004c38e4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x2a4>
1004c38dc:      add w27, w10, #0x2
1004c38e0:      add w24, w9, #0x2
1004c38e4:      ldr x8, [sp, #0x28]
1004c38e8:      ldr x21, [x8, x23, lsl #3]
1004c38ec:      sub x0, x29, #0xc8
1004c38f0:      mov x1, x21
1004c38f4:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c38f8:      ldur    w8, [x29, #-0xc8]
1004c38fc:      cmn w8, #0x1
1004c3900:      b.eq    0x1004c395c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x31c>
1004c3904:      ldp w20, w21, [x29, #-0xc4]
1004c3908:      ldur    w9, [x29, #-0xbc]
1004c390c:      add x10, sp, #0x180
1004c3910:      madd    x10, x23, x25, x10
1004c3914:      stp w8, w20, [x10]
1004c3918:      stp w21, w9, [x10, #0x8]
1004c391c:      sub x11, x29, #0xc8
1004c3920:      ldur    q0, [x11, #0x10]
1004c3924:      str q0, [x10, #0x10]
1004c3928:      ldur    x11, [x11, #0x20]
1004c392c:      str x11, [x10, #0x20]
1004c3930:      cmp w8, #0x2
1004c3934:      b.eq    0x1004c3af0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4b0>
1004c3938:      cmp w8, #0x1
1004c393c:      b.ne    0x1004c3b0c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4cc>
1004c3940:      mov x20, x9
1004c3944:      cmp x23, #0x0
1004c3948:      mov w8, #0x1                ; =1
1004c394c:      cinc    w8, w8, ne
1004c3950:      adds    w9, w27, w22
1004c3954:      b.lo    0x1004c3b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3958:      b   0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c395c:      mov w8, #0x7ffd             ; =32765
1004c3960:      cmp x8, x21, lsr #48
1004c3964:      b.ne    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3968:      and x21, x21, #0xffffffffffff
1004c396c:      mov x0, x21
1004c3970:      bl  0x100535c04 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1004c3974:      cbz x0, 0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3978:      ldrb    w8, [x0]
1004c397c:      cmp w8, #0x1
1004c3980:      b.ne    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3984:      ldrsb   w8, [x0, #0x1]
1004c3988:      tbnz    w8, #0x1f, 0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c398c:      ldrh    w8, [x0, #0x2]
1004c3990:      tbnz    w8, #0xa, 0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3994:      ldr w8, [x0, #0x4]
1004c3998:      cmp w8, #0x10
1004c399c:      b.lo    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39a0:      mov x20, x19
1004c39a4:      ldr w19, [x21]
1004c39a8:      cmp w19, #0x10
1004c39ac:      b.hi    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39b0:      ldr w9, [x21, #0x4]
1004c39b4:      cmp w19, w9
1004c39b8:      b.hi    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39bc:      str w27, [sp, #0x20]
1004c39c0:      lsl x27, x19, #3
1004c39c4:      add x9, x27, #0x10
1004c39c8:      cmp x9, x8
1004c39cc:      b.hi    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39d0:      mov x0, x21
1004c39d4:      bl  0x1004fb4ac <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime5array6header35array_has_named_properties_resolved>
1004c39d8:      tbnz    w0, #0x0, 0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39dc:      mov x0, x21
1004c39e0:      bl  0x100567308 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime6object15prototype_chain23object_static_prototype>
1004c39e4:      cmp x0, #0x1
1004c39e8:      b.eq    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39ec:      str w24, [sp, #0x1c]
1004c39f0:      add x24, x20, x19
1004c39f4:      cmp x24, #0x10
1004c39f8:      b.hi    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c39fc:      add x8, sp, #0x180
1004c3a00:      madd    x8, x23, x25, x8
1004c3a04:      mov w9, #-0x1               ; =-1
1004c3a08:      str w9, [x8]
1004c3a0c:      stp x20, x19, [x8, #0x8]
1004c3a10:      cbz w19, 0x1004c3b30 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4f0>
1004c3a14:      str w22, [sp, #0x4]
1004c3a18:      mov w9, #0x28               ; =40
1004c3a1c:      mov x25, #0x0               ; =0
1004c3a20:      add x19, x21, #0x8
1004c3a24:      ldr x8, [sp, #0x10]
1004c3a28:      madd    x22, x20, x9, x8
1004c3a2c:      mov w20, #0x2               ; =2
1004c3a30:      mov w21, #0x2               ; =2
1004c3a34:      ldr x1, [x19, x25]
1004c3a38:      sub x0, x29, #0x80
1004c3a3c:      bl  0x1004bd3f0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat12scalar_piece>
1004c3a40:      ldur    w8, [x29, #-0x80]
1004c3a44:      cmn w8, #0x1
1004c3a48:      b.eq    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3a4c:      ldp w10, w9, [x29, #-0x7c]
1004c3a50:      ldur    w11, [x29, #-0x74]
1004c3a54:      ldur    q0, [x28, #0x10]
1004c3a58:      stur    q0, [x29, #-0xa0]
1004c3a5c:      ldur    x12, [x28, #0x20]
1004c3a60:      stur    x12, [x29, #-0x90]
1004c3a64:      stp w8, w10, [x22, #-0x8]
1004c3a68:      stp w9, w11, [x22]
1004c3a6c:      stur    q0, [x22, #0x8]
1004c3a70:      str x12, [x22, #0x18]
1004c3a74:      cbz w8, 0x1004c3a98 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x458>
1004c3a78:      cmp w8, #0x1
1004c3a7c:      csel    w8, w11, w10, eq
1004c3a80:      csel    w10, w9, w10, eq
1004c3a84:      cmp x25, #0x0
1004c3a88:      cset    w9, ne
1004c3a8c:      adds    w10, w10, w21
1004c3a90:      b.lo    0x1004c3ab0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x470>
1004c3a94:      b   0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3a98:      add w10, w10, #0x2
1004c3a9c:      add w8, w9, #0x2
1004c3aa0:      cmp x25, #0x0
1004c3aa4:      cset    w9, ne
1004c3aa8:      adds    w10, w10, w21
1004c3aac:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ab0:      adds    w21, w10, w9
1004c3ab4:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3ab8:      mov x0, #0x0                ; =0
1004c3abc:      adds    w8, w8, w20
1004c3ac0:      cset    w10, hs
1004c3ac4:      adds    w20, w8, w9
1004c3ac8:      cset    w8, hs
1004c3acc:      tbnz    w10, #0x0, 0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3ad0:      tbnz    w8, #0x0, 0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3ad4:      add x25, x25, #0x8
1004c3ad8:      add x22, x22, #0x28
1004c3adc:      cmp x27, x25
1004c3ae0:      b.ne    0x1004c3a34 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x3f4>
1004c3ae4:      mov x19, x24
1004c3ae8:      ldr w22, [sp, #0x4]
1004c3aec:      b   0x1004c3b3c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x4fc>
1004c3af0:      mov x21, x20
1004c3af4:      cmp x23, #0x0
1004c3af8:      mov w8, #0x1                ; =1
1004c3afc:      cinc    w8, w8, ne
1004c3b00:      adds    w9, w27, w22
1004c3b04:      b.lo    0x1004c3b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3b08:      b   0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b0c:      add w8, w20, #0x2
1004c3b10:      add w20, w21, #0x2
1004c3b14:      mov x21, x8
1004c3b18:      cmp x23, #0x0
1004c3b1c:      mov w8, #0x1                ; =1
1004c3b20:      cinc    w8, w8, ne
1004c3b24:      adds    w9, w27, w22
1004c3b28:      b.lo    0x1004c3b54 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x514>
1004c3b2c:      b   0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b30:      mov w20, #0x2               ; =2
1004c3b34:      mov w21, #0x2               ; =2
1004c3b38:      mov x19, x24
1004c3b3c:      ldp w24, w27, [sp, #0x1c]
1004c3b40:      cmp x23, #0x0
1004c3b44:      mov w8, #0x1                ; =1
1004c3b48:      cinc    w8, w8, ne
1004c3b4c:      adds    w9, w27, w22
1004c3b50:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b54:      adds    w9, w21, w9
1004c3b58:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b5c:      adds    w11, w9, w8
1004c3b60:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b64:      adds    w9, w24, w26
1004c3b68:      b.hs    0x1004c3c78 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x638>
1004c3b6c:      mov x0, #0x0                ; =0
1004c3b70:      adds    w9, w20, w9
1004c3b74:      cset    w10, hs
1004c3b78:      adds    w26, w9, w8
1004c3b7c:      cset    w8, hs
1004c3b80:      tbnz    w10, #0x0, 0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3b84:      tbnz    w8, #0x0, 0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3b88:      add x23, x23, #0x1
1004c3b8c:      ldr x8, [sp, #0x38]
1004c3b90:      cmp x23, x8
1004c3b94:      mov x22, x11
1004c3b98:      mov w25, #0x28              ; =40
1004c3b9c:      b.ne    0x1004c387c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x23c>
1004c3ba0:      bl  0x10026a350 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope3new>
1004c3ba4:      mov x9, #0x7ffd000000000000 ; =9222527611924643840
1004c3ba8:      mov w8, #0x1                ; =1
1004c3bac:      ldr x19, [sp, #0x8]
1004c3bb0:      stp x19, x9, [x29, #-0x78]
1004c3bb4:      stp x0, x8, [x29, #-0x88]
1004c3bb8:      sub x0, x29, #0x80
1004c3bbc:      bl  0x10026a458 <__RNvMs_NtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
1004c3bc0:      mov x23, x0
1004c3bc4:      adrp    x0, 0x100f01000 <__MergedGlobals+0x100>
1004c3bc8:      add x0, x0, #0xbf0
1004c3bcc:      ldr x8, [x0]
1004c3bd0:      blr x8
1004c3bd4:      strb    wzr, [x0]
1004c3bd8:      mov x0, x19
1004c3bdc:      bl  0x1004c351c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json22stringify_tojson_probe40to_json_definitely_absent_after_own_keys>
1004c3be0:      tbz w0, #0x0, 0x1004c3c70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x630>
1004c3be4:      mov x0, x22
1004c3be8:      bl  0x10032570c <__RNvNtCs7wa3bwJW6LN_13perry_runtime6string20string_storage_alloc>
1004c3bec:      mov x21, x1
1004c3bf0:      stp w26, w22, [x0]
1004c3bf4:      stp wzr, wzr, [x0, #0xc]
1004c3bf8:      str w22, [x0, #0x8]
1004c3bfc:      adrp    x24, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3c00:      ldr x8, [x24, #0x48]
1004c3c04:      cmn x8, #0x1
1004c3c08:      str x0, [sp, #0x20]
1004c3c0c:      b.eq    0x1004c3c9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3c10:      mrs x9, TPIDRRO_EL0
1004c3c14:      and x9, x9, #0xfffffffffffffff8
1004c3c18:      ldr x8, [x9, x8, lsl #3]
1004c3c1c:      cbz x8, 0x1004c3c9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3c20:      ldr x8, [x8, #0x19e8]
1004c3c24:      cbz x8, 0x1004c3c9c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x65c>
1004c3c28:      ldr x9, [x8]
1004c3c2c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
1004c3c30:      cmp x9, x10
1004c3c34:      b.hs    0x1004c3f50 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x910>
1004c3c38:      add x10, x9, #0x1
1004c3c3c:      str x10, [x8]
1004c3c40:      ldr x10, [x8, #0x18]
1004c3c44:      cmp x23, x10
1004c3c48:      b.hs    0x1004c3f5c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1004c3c4c:      ldr x10, [x8, #0x10]
1004c3c50:      mov w11, #0x18              ; =24
1004c3c54:      madd    x10, x23, x11, x10
1004c3c58:      ldr x11, [x10]
1004c3c5c:      cmp x11, #0x1
1004c3c60:      b.ne    0x1004c3f60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x920>
1004c3c64:      ldr x0, [x10, #0x8]
1004c3c68:      str x9, [x8]
1004c3c6c:      b   0x1004c3ca4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x664>
1004c3c70:      sub x0, x29, #0x88
1004c3c74:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3c78:      mov x0, #0x0                ; =0
1004c3c7c:      add sp, sp, #0x5d0
1004c3c80:      ldp x29, x30, [sp, #0x50]
1004c3c84:      ldp x20, x19, [sp, #0x40]
1004c3c88:      ldp x22, x21, [sp, #0x30]
1004c3c8c:      ldp x24, x23, [sp, #0x20]
1004c3c90:      ldp x26, x25, [sp, #0x10]
1004c3c94:      ldp x28, x27, [sp], #0x60
1004c3c98:      ret
1004c3c9c:      mov x0, x23
1004c3ca0:      bl  0x10013ba14 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotONtNtB28_3map9MapHeaderNCINvB3g_15get_raw_mut_ptrB4d_E0E0B4c_EB28_>
1004c3ca4:      adrp    x9, 0x100efc000 <_perry_global_baseline_worker_ts__1>
1004c3ca8:      ldr w20, [x0, #0x4]
1004c3cac:      mov w8, #-0x40000000        ; =-1073741824
1004c3cb0:      cmp w20, w8
1004c3cb4:      csel    w19, w20, w8, lt
1004c3cb8:      ldr w22, [x9, #0x634]
1004c3cbc:      cmp w22, #0x300
1004c3cc0:      b.hs    0x1004c3d60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c3cc4:      ldr x8, [x24, #0x48]
1004c3cc8:      cmn x8, #0x1
1004c3ccc:      b.eq    0x1004c3d44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3cd0:      mrs x9, TPIDRRO_EL0
1004c3cd4:      and x9, x9, #0xfffffffffffffff8
1004c3cd8:      ldr x8, [x9, x8, lsl #3]
1004c3cdc:      cbz x8, 0x1004c3d44 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x704>
1004c3ce0:      add x8, x8, x22, lsl #3
1004c3ce4:      ldr x8, [x8, #0x1e8]
1004c3ce8:      cbz x8, 0x1004c3d60 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x720>
1004c3cec:      ldr x8, [x8]
1004c3cf0:      cbz x8, 0x1004c3d84 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x744>
1004c3cf4:      mov w9, #-0x40000001        ; =-1073741825
1004c3cf8:      cmp w20, w9
1004c3cfc:      b.gt    0x1004c3da0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3d00:      ldr x11, [x8, #0x5198]
1004c3d04:      and w9, w19, #0x3fffffff
1004c3d08:      lsr x10, x9, #15
1004c3d0c:      cmp x10, x11
1004c3d10:      b.hs    0x1004c3da0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3d14:      ldr x8, [x8, #0x5190]
1004c3d18:      ldr x8, [x8, x10, lsl #3]
1004c3d1c:      cbz x8, 0x1004c3da4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3d20:      ubfx    x10, x9, #5, #10
1004c3d24:      ldr x8, [x8, x10, lsl #3]
1004c3d28:      cbz x8, 0x1004c3da4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3d2c:      and x9, x9, #0x1f
1004c3d30:      add x8, x8, x9, lsl #5
1004c3d34:      ldrb    w9, [x8, #0x1c]
1004c3d38:      tbz w9, #0x0, 0x1004c3da0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x760>
1004c3d3c:      ldr x8, [x8]
1004c3d40:      b   0x1004c3da4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x764>
1004c3d44:      mov x23, x0
1004c3d48:      bl  0x100adbcf0 <__RNvNtCs7wa3bwJW6LN_13perry_runtime7tls_hot12hot_uncached>
1004c3d4c:      mov x8, x0
1004c3d50:      mov x0, x23
1004c3d54:      add x8, x8, x22, lsl #3
1004c3d58:      ldr x8, [x8, #0x1e8]
1004c3d5c:      cbnz    x8, 0x1004c3cec <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6ac>
1004c3d60:      adrp    x8, 0x100e89000 <__RNvNtCs7wa3bwJW6LN_13perry_runtime4yoga21GC_SCANNER_REGISTERED+0x1e0>
1004c3d64:      add x8, x8, #0x330
1004c3d68:      mov x22, x0
1004c3d6c:      mov x0, x8
1004c3d70:      bl  0x100ad915c <__RNvMs5_NtCs7wa3bwJW6LN_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1004c3d74:      mov x8, x0
1004c3d78:      mov x0, x22
1004c3d7c:      ldr x8, [x8]
1004c3d80:      cbnz    x8, 0x1004c3cf4 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6b4>
1004c3d84:      mov x22, x0
1004c3d88:      bl  0x100adae2c <__RNvNtCs7wa3bwJW6LN_13perry_runtime5state10init_state>
1004c3d8c:      mov x8, x0
1004c3d90:      mov x0, x22
1004c3d94:      mov w9, #-0x40000001        ; =-1073741825
1004c3d98:      cmp w20, w9
1004c3d9c:      b.le    0x1004c3d00 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x6c0>
1004c3da0:      mov x8, #0x0                ; =0
1004c3da4:      mov x19, #0x0               ; =0
1004c3da8:      mov w9, #0x7b               ; =123
1004c3dac:      strb    w9, [x21]
1004c3db0:      add x25, x8, #0x8
1004c3db4:      add x24, x0, #0x10
1004c3db8:      add x8, sp, #0x2c0
1004c3dbc:      add x8, x8, #0x28
1004c3dc0:      stp x8, x25, [sp, #0x28]
1004c3dc4:      mov w22, #0x1               ; =1
1004c3dc8:      add x27, sp, #0x40
1004c3dcc:      mov w28, #0x3a              ; =58
1004c3dd0:      mov w20, #0x2c              ; =44
1004c3dd4:      b   0x1004c3dfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c3dd8:      mov w8, #0x5d               ; =93
1004c3ddc:      strb    w8, [x21, x25]
1004c3de0:      add x22, x25, #0x1
1004c3de4:      ldp x25, x8, [sp, #0x30]
1004c3de8:      add x27, sp, #0x40
1004c3dec:      mov w28, #0x3a              ; =58
1004c3df0:      add x19, x19, #0x1
1004c3df4:      cmp x19, x8
1004c3df8:      b.eq    0x1004c3f28 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c3dfc:      cbz x19, 0x1004c3e08 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7c8>
1004c3e00:      strb    w20, [x21, x22]
1004c3e04:      add x22, x22, #0x1
1004c3e08:      add x8, x19, x19, lsl #2
1004c3e0c:      lsl x23, x8, #3
1004c3e10:      ldr x1, [x25, x19, lsl #3]
1004c3e14:      add x0, x27, x23
1004c3e18:      add x2, x21, x22
1004c3e1c:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3e20:      add x8, x0, x22
1004c3e24:      strb    w28, [x21, x8]
1004c3e28:      add x26, x8, #0x1
1004c3e2c:      ldr x1, [x24, x19, lsl #3]
1004c3e30:      add x9, sp, #0x180
1004c3e34:      add x0, x9, x23
1004c3e38:      ldr w9, [x0]
1004c3e3c:      cmn w9, #0x1
1004c3e40:      b.eq    0x1004c3e64 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x824>
1004c3e44:      add x2, x21, x26
1004c3e48:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3e4c:      add x22, x0, x26
1004c3e50:      add x19, x19, #0x1
1004c3e54:      ldr x8, [sp, #0x38]
1004c3e58:      cmp x19, x8
1004c3e5c:      b.ne    0x1004c3dfc <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x7bc>
1004c3e60:      b   0x1004c3f28 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8e8>
1004c3e64:      ldp x23, x22, [x0, #0x8]
1004c3e68:      add x25, x8, #0x2
1004c3e6c:      mov w8, #0x5b               ; =91
1004c3e70:      strb    w8, [x21, x26]
1004c3e74:      cbz x22, 0x1004c3dd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c3e78:      cmp x23, #0xf
1004c3e7c:      b.hi    0x1004c3f70 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x930>
1004c3e80:      and x26, x1, #0xffffffffffff
1004c3e84:      add x8, sp, #0x2c0
1004c3e88:      mov w9, #0x28               ; =40
1004c3e8c:      madd    x8, x23, x9, x8
1004c3e90:      ldp q0, q1, [x8]
1004c3e94:      stp q0, q1, [x29, #-0x80]
1004c3e98:      ldr x8, [x8, #0x20]
1004c3e9c:      stur    x8, [x29, #-0x60]
1004c3ea0:      ldr x1, [x26, #0x8]
1004c3ea4:      sub x0, x29, #0x80
1004c3ea8:      add x2, x21, x25
1004c3eac:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3eb0:      add x25, x0, x25
1004c3eb4:      subs    x27, x22, #0x1
1004c3eb8:      b.eq    0x1004c3dd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c3ebc:      mov w9, #0x28               ; =40
1004c3ec0:      add x28, x26, #0x10
1004c3ec4:      cmp x23, #0x10
1004c3ec8:      mov w8, #0x10               ; =16
1004c3ecc:      csel    x8, x23, x8, lo
1004c3ed0:      sub x26, x8, #0xf
1004c3ed4:      add x22, x23, #0x1
1004c3ed8:      ldr x8, [sp, #0x28]
1004c3edc:      madd    x23, x23, x9, x8
1004c3ee0:      strb    w20, [x21, x25]
1004c3ee4:      cbz x26, 0x1004c3f74 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x934>
1004c3ee8:      add x25, x25, #0x1
1004c3eec:      ldp q0, q1, [x23]
1004c3ef0:      stp q0, q1, [x29, #-0x80]
1004c3ef4:      ldr x8, [x23, #0x20]
1004c3ef8:      stur    x8, [x29, #-0x60]
1004c3efc:      ldr x1, [x28], #0x8
1004c3f00:      sub x0, x29, #0x80
1004c3f04:      add x2, x21, x25
1004c3f08:      bl  0x1004bc8a8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json14stringify_flat10emit_piece>
1004c3f0c:      add x25, x0, x25
1004c3f10:      add x26, x26, #0x1
1004c3f14:      add x22, x22, #0x1
1004c3f18:      add x23, x23, #0x28
1004c3f1c:      subs    x27, x27, #0x1
1004c3f20:      b.ne    0x1004c3ee0 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x8a0>
1004c3f24:      b   0x1004c3dd8 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x798>
1004c3f28:      mov w8, #0x7d               ; =125
1004c3f2c:      strb    w8, [x21, x22]
1004c3f30:      mov x19, #0x7fff000000000000 ; =9223090561878065152
1004c3f34:      ldr x8, [sp, #0x20]
1004c3f38:      bfxil   x19, x8, #0, #48
1004c3f3c:      sub x0, x29, #0x88
1004c3f40:      bl  0x100160f4c <__RINvNtCsjgY6bXVaRmE_4core3ptr9drop_glueNtNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles18RuntimeHandleScopeEBJ_>
1004c3f44:      mov x1, x19
1004c3f48:      mov w0, #0x1                ; =1
1004c3f4c:      b   0x1004c3c7c <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime4json23stringify_record_output11emit_record+0x63c>
1004c3f50:      adrp    x0, 0x100e77000 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime7closure8dispatch5bound21KEEP_JS_FUNCTION_BIND+0x1fc8>
1004c3f54:      add x0, x0, #0x8a8
1004c3f58:      bl  0x100ab131c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1004c3f5c:      bl  0x100ae2bc8 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
1004c3f60:      adrp    x0, 0x100bd9000 <_PERRY_EMPTY_STRING+0xbb0>
1004c3f64:      add x0, x0, #0xac
1004c3f68:      mov w1, #0xb                ; =11
1004c3f6c:      bl  0x100ae2b90 <__RNvNtNtNtCs7wa3bwJW6LN_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1004c3f70:      mov x22, x23
1004c3f74:      adrp    x2, 0x100e8e000 <__RNvNtNtCs7wa3bwJW6LN_13perry_runtime2os18os_process_emitter26PROCESS_EXIT_EVENT_EMITTED+0x2ef0>
1004c3f78:      add x2, x2, #0x0
1004c3f7c:      mov x0, x22
1004c3f80:      mov w1, #0x10               ; =16
1004c3f84:      bl  0x100ab144c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
