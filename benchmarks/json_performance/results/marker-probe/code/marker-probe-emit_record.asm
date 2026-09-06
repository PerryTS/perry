/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/marker-probe-worker: file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100755700 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record>:
100755700:      stp x28, x27, [sp, #-0x60]!
100755704:      stp x26, x25, [sp, #0x10]
100755708:      stp x24, x23, [sp, #0x20]
10075570c:      stp x22, x21, [sp, #0x30]
100755710:      stp x20, x19, [sp, #0x40]
100755714:      stp x29, x30, [sp, #0x50]
100755718:      add x29, sp, #0x50
10075571c:      sub sp, sp, #0x5e0
100755720:      ldr xzr, [sp]
100755724:      str x1, [sp, #0x38]
100755728:      mov x10, x0
10075572c:      movi.2d v0, #0000000000000000
100755730:      str d0, [sp, #0x40]
100755734:      str wzr, [sp, #0x48]
100755738:      str d0, [sp, #0x68]
10075573c:      str wzr, [sp, #0x70]
100755740:      str d0, [sp, #0x90]
100755744:      str wzr, [sp, #0x98]
100755748:      str d0, [sp, #0xb8]
10075574c:      str wzr, [sp, #0xc0]
100755750:      str d0, [sp, #0xe0]
100755754:      str wzr, [sp, #0xe8]
100755758:      str d0, [sp, #0x108]
10075575c:      str wzr, [sp, #0x110]
100755760:      str d0, [sp, #0x130]
100755764:      str wzr, [sp, #0x138]
100755768:      str d0, [sp, #0x158]
10075576c:      str wzr, [sp, #0x160]
100755770:      str d0, [sp, #0x180]
100755774:      str wzr, [sp, #0x188]
100755778:      str d0, [sp, #0x1a8]
10075577c:      str wzr, [sp, #0x1b0]
100755780:      str d0, [sp, #0x1d0]
100755784:      str wzr, [sp, #0x1d8]
100755788:      str d0, [sp, #0x1f8]
10075578c:      str wzr, [sp, #0x200]
100755790:      str d0, [sp, #0x220]
100755794:      str wzr, [sp, #0x228]
100755798:      str d0, [sp, #0x248]
10075579c:      str wzr, [sp, #0x250]
1007557a0:      str d0, [sp, #0x270]
1007557a4:      str wzr, [sp, #0x278]
1007557a8:      str d0, [sp, #0x298]
1007557ac:      str wzr, [sp, #0x2a0]
1007557b0:      str d0, [sp, #0x2c0]
1007557b4:      str wzr, [sp, #0x2c8]
1007557b8:      str d0, [sp, #0x2e8]
1007557bc:      str wzr, [sp, #0x2f0]
1007557c0:      str d0, [sp, #0x310]
1007557c4:      str wzr, [sp, #0x318]
1007557c8:      str d0, [sp, #0x338]
1007557cc:      str wzr, [sp, #0x340]
1007557d0:      str d0, [sp, #0x360]
1007557d4:      str wzr, [sp, #0x368]
1007557d8:      str d0, [sp, #0x388]
1007557dc:      str wzr, [sp, #0x390]
1007557e0:      str d0, [sp, #0x3b0]
1007557e4:      str wzr, [sp, #0x3b8]
1007557e8:      str d0, [sp, #0x3d8]
1007557ec:      str wzr, [sp, #0x3e0]
1007557f0:      str d0, [sp, #0x400]
1007557f4:      str wzr, [sp, #0x408]
1007557f8:      str d0, [sp, #0x428]
1007557fc:      str wzr, [sp, #0x430]
100755800:      str d0, [sp, #0x450]
100755804:      str wzr, [sp, #0x458]
100755808:      str d0, [sp, #0x478]
10075580c:      str wzr, [sp, #0x480]
100755810:      str d0, [sp, #0x4a0]
100755814:      str wzr, [sp, #0x4a8]
100755818:      str d0, [sp, #0x4c8]
10075581c:      str wzr, [sp, #0x4d0]
100755820:      str d0, [sp, #0x4f0]
100755824:      str wzr, [sp, #0x4f8]
100755828:      str d0, [sp, #0x518]
10075582c:      str wzr, [sp, #0x520]
100755830:      ldr w19, [x0, #0x4]
100755834:      adrp    x8, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100755838:      add x8, x8, #0x94
10075583c:      ldr w20, [x8]
100755840:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
100755844:      add x8, x8, #0x768
100755848:      cmp w20, #0x300
10075584c:      b.hs    0x1007558e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
100755850:      ldr x8, [x8]
100755854:      cmn x8, #0x1
100755858:      b.eq    0x1007558cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
10075585c:      mrs x9, TPIDRRO_EL0
100755860:      and x9, x9, #0xfffffffffffffff8
100755864:      ldr x0, [x9, x8, lsl #3]
100755868:      cbz x0, 0x1007558cc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
10075586c:      add x8, x0, x20, lsl #3
100755870:      ldr x0, [x8, #0x1e8]
100755874:      cbz x0, 0x1007558e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
100755878:      ldr x0, [x0]
10075587c:      cbz x0, 0x100755900 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x200>
100755880:      mov w8, #-0x40000001        ; =-1073741825
100755884:      cmp w19, w8
100755888:      b.gt    0x100755918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
10075588c:      ldr x9, [x0, #0x5198]
100755890:      ubfx    x8, x19, #15, #15
100755894:      cmp x8, x9
100755898:      b.hs    0x100755918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
10075589c:      ldr x9, [x0, #0x5190]
1007558a0:      ldr x8, [x9, x8, lsl #3]
1007558a4:      cbz x8, 0x10075591c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1007558a8:      ubfx    x9, x19, #5, #10
1007558ac:      ldr x8, [x8, x9, lsl #3]
1007558b0:      cbz x8, 0x10075591c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1007558b4:      and x9, x19, #0x1f
1007558b8:      add x8, x8, x9, lsl #5
1007558bc:      ldrb    w9, [x8, #0x1c]
1007558c0:      tbz w9, #0x0, 0x100755918 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
1007558c4:      ldr x8, [x8]
1007558c8:      b   0x10075591c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
1007558cc:      mov x21, x10
1007558d0:      bl  0x100cc8104 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1007558d4:      mov x10, x21
1007558d8:      add x8, x0, x20, lsl #3
1007558dc:      ldr x0, [x8, #0x1e8]
1007558e0:      cbnz    x0, 0x100755878 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x178>
1007558e4:      adrp    x0, 0x1010d9000 <_anon.72cde5cdc14742b721629e115e16bf6f.1612+0x158>
1007558e8:      add x0, x0, #0x288
1007558ec:      mov x20, x10
1007558f0:      bl  0x100cc7950 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1007558f4:      mov x10, x20
1007558f8:      ldr x0, [x0]
1007558fc:      cbnz    x0, 0x100755880 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x180>
100755900:      mov x20, x10
100755904:      bl  0x100cc7e88 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
100755908:      mov x10, x20
10075590c:      mov w8, #-0x40000001        ; =-1073741825
100755910:      cmp w19, w8
100755914:      b.le    0x10075588c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x18c>
100755918:      mov x8, #0x0                ; =0
10075591c:      mov x23, #0x0               ; =0
100755920:      add x8, x8, #0x8
100755924:      str x8, [sp, #0x30]
100755928:      sub x28, x29, #0x80
10075592c:      add x8, x10, #0x10
100755930:      stp xzr, x8, [sp, #0x20]
100755934:      add x8, sp, #0x2c0
100755938:      add x8, x8, #0x8
10075593c:      stp x10, x8, [sp, #0x10]
100755940:      mov w22, #0x2               ; =2
100755944:      mov w27, #0x28              ; =40
100755948:      mov w26, #0x2               ; =2
10075594c:      ldr x8, [sp, #0x30]
100755950:      ldr x1, [x8, x23, lsl #3]
100755954:      sub x0, x29, #0x80
100755958:      bl  0x1007500d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
10075595c:      ldur    w9, [x29, #-0x80]
100755960:      cmn w9, #0x1
100755964:      b.eq    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755968:      ldp w8, w19, [x29, #-0x7c]
10075596c:      ldur    w25, [x29, #-0x74]
100755970:      ldur    q0, [x28, #0x10]
100755974:      stur    q0, [x29, #-0xf0]
100755978:      ldur    x10, [x28, #0x20]
10075597c:      stur    x10, [x29, #-0xe0]
100755980:      add x11, sp, #0x40
100755984:      madd    x11, x23, x27, x11
100755988:      stp w9, w8, [x11]
10075598c:      stp w19, w25, [x11, #0x8]
100755990:      str q0, [x11, #0x10]
100755994:      str x10, [x11, #0x20]
100755998:      cmp w9, #0x2
10075599c:      b.eq    0x1007559b4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2b4>
1007559a0:      cmp w9, #0x1
1007559a4:      b.eq    0x1007559bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
1007559a8:      add w25, w19, #0x2
1007559ac:      add w19, w8, #0x2
1007559b0:      b   0x1007559bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
1007559b4:      mov x25, x8
1007559b8:      mov x19, x8
1007559bc:      ldr x8, [sp, #0x28]
1007559c0:      ldr x21, [x8, x23, lsl #3]
1007559c4:      sub x0, x29, #0xd8
1007559c8:      mov x1, x21
1007559cc:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1007559d0:      ldur    w8, [x29, #-0xd8]
1007559d4:      cmn w8, #0x1
1007559d8:      b.eq    0x100755a34 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x334>
1007559dc:      ldp w20, w21, [x29, #-0xd4]
1007559e0:      ldur    w9, [x29, #-0xcc]
1007559e4:      add x10, sp, #0x180
1007559e8:      madd    x10, x23, x27, x10
1007559ec:      stp w8, w20, [x10]
1007559f0:      stp w21, w9, [x10, #0x8]
1007559f4:      sub x11, x29, #0xd8
1007559f8:      ldur    q0, [x11, #0x10]
1007559fc:      str q0, [x10, #0x10]
100755a00:      ldur    x11, [x11, #0x20]
100755a04:      str x11, [x10, #0x20]
100755a08:      cmp w8, #0x2
100755a0c:      b.eq    0x100755be4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x4e4>
100755a10:      cmp w8, #0x1
100755a14:      b.ne    0x100755c00 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x500>
100755a18:      mov x20, x9
100755a1c:      cmp x23, #0x0
100755a20:      mov w8, #0x1                ; =1
100755a24:      cinc    w8, w8, ne
100755a28:      adds    w9, w19, w22
100755a2c:      b.lo    0x100755c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
100755a30:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a34:      mov w8, #0x7ffd             ; =32765
100755a38:      cmp x8, x21, lsr #48
100755a3c:      b.ne    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a40:      and x21, x21, #0xffffffffffff
100755a44:      mov x0, x21
100755a48:      bl  0x100773f1c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
100755a4c:      cbz x0, 0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100755a50:      ldrb    w8, [x0]
100755a54:      cmp w8, #0x1
100755a58:      b.ne    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a5c:      ldrsb   w8, [x0, #0x1]
100755a60:      tbnz    w8, #0x1f, 0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a64:      ldrh    w8, [x0, #0x2]
100755a68:      tbnz    w8, #0xa, 0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a6c:      ldr w8, [x0, #0x4]
100755a70:      cmp w8, #0x10
100755a74:      b.lo    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a78:      ldr w20, [x21]
100755a7c:      cmp w20, #0x10
100755a80:      b.hi    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a84:      ldr w9, [x21, #0x4]
100755a88:      cmp w20, w9
100755a8c:      b.hi    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755a90:      lsl x24, x20, #3
100755a94:      add x9, x24, #0x10
100755a98:      cmp x9, x8
100755a9c:      b.hi    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755aa0:      mov x0, x21
100755aa4:      bl  0x100772524 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
100755aa8:      tbnz    w0, #0x0, 0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755aac:      mov x0, x21
100755ab0:      bl  0x100378564 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
100755ab4:      cmp x0, #0x1
100755ab8:      b.eq    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755abc:      ldr x8, [sp, #0x20]
100755ac0:      add x10, x8, x20
100755ac4:      cmp x10, #0x10
100755ac8:      b.hi    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755acc:      add x8, sp, #0x180
100755ad0:      madd    x8, x23, x27, x8
100755ad4:      mov w9, #-0x1               ; =-1
100755ad8:      str w9, [x8]
100755adc:      ldr x9, [sp, #0x20]
100755ae0:      stp x9, x20, [x8, #0x8]
100755ae4:      cbz w20, 0x100755c24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x524>
100755ae8:      str x10, [sp]
100755aec:      stp w26, w22, [sp, #0x8]
100755af0:      mov x22, #0x0               ; =0
100755af4:      mov x10, x9
100755af8:      mov w9, #0x28               ; =40
100755afc:      add x27, x21, #0x8
100755b00:      ldr x8, [sp, #0x18]
100755b04:      madd    x26, x10, x9, x8
100755b08:      mov w20, #0x2               ; =2
100755b0c:      mov w21, #0x2               ; =2
100755b10:      ldr x1, [x27, x22]
100755b14:      sub x0, x29, #0x80
100755b18:      bl  0x10074fe0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
100755b1c:      ldur    w8, [x29, #-0x80]
100755b20:      cmn w8, #0x1
100755b24:      b.eq    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755b28:      ldp w10, w9, [x29, #-0x7c]
100755b2c:      ldur    w11, [x29, #-0x74]
100755b30:      ldur    q0, [x28, #0x10]
100755b34:      stur    q0, [x29, #-0xb0]
100755b38:      ldur    x12, [x28, #0x20]
100755b3c:      stur    x12, [x29, #-0xa0]
100755b40:      stp w8, w10, [x26, #-0x8]
100755b44:      stp w9, w11, [x26]
100755b48:      stur    q0, [x26, #0x8]
100755b4c:      str x12, [x26, #0x18]
100755b50:      cbz w8, 0x100755b74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x474>
100755b54:      cmp w8, #0x1
100755b58:      csel    w8, w11, w10, eq
100755b5c:      csel    w10, w9, w10, eq
100755b60:      cmp x22, #0x0
100755b64:      cset    w9, ne
100755b68:      adds    w10, w10, w21
100755b6c:      b.lo    0x100755b8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x48c>
100755b70:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755b74:      add w10, w10, #0x2
100755b78:      add w8, w9, #0x2
100755b7c:      cmp x22, #0x0
100755b80:      cset    w9, ne
100755b84:      adds    w10, w10, w21
100755b88:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755b8c:      adds    w21, w10, w9
100755b90:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755b94:      mov x0, #0x0                ; =0
100755b98:      adds    w8, w8, w20
100755b9c:      cset    w10, hs
100755ba0:      adds    w20, w8, w9
100755ba4:      cset    w8, hs
100755ba8:      tbnz    w10, #0x0, 0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100755bac:      tbnz    w8, #0x0, 0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100755bb0:      add x22, x22, #0x8
100755bb4:      add x26, x26, #0x28
100755bb8:      cmp x24, x22
100755bbc:      b.ne    0x100755b10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x410>
100755bc0:      ldr x8, [sp]
100755bc4:      str x8, [sp, #0x20]
100755bc8:      ldp w26, w22, [sp, #0x8]
100755bcc:      cmp x23, #0x0
100755bd0:      mov w8, #0x1                ; =1
100755bd4:      cinc    w8, w8, ne
100755bd8:      adds    w9, w19, w22
100755bdc:      b.lo    0x100755c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
100755be0:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755be4:      mov x21, x20
100755be8:      cmp x23, #0x0
100755bec:      mov w8, #0x1                ; =1
100755bf0:      cinc    w8, w8, ne
100755bf4:      adds    w9, w19, w22
100755bf8:      b.lo    0x100755c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
100755bfc:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c00:      add w8, w20, #0x2
100755c04:      add w20, w21, #0x2
100755c08:      mov x21, x8
100755c0c:      cmp x23, #0x0
100755c10:      mov w8, #0x1                ; =1
100755c14:      cinc    w8, w8, ne
100755c18:      adds    w9, w19, w22
100755c1c:      b.lo    0x100755c44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
100755c20:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c24:      mov w20, #0x2               ; =2
100755c28:      mov w21, #0x2               ; =2
100755c2c:      str x10, [sp, #0x20]
100755c30:      cmp x23, #0x0
100755c34:      mov w8, #0x1                ; =1
100755c38:      cinc    w8, w8, ne
100755c3c:      adds    w9, w19, w22
100755c40:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c44:      adds    w9, w21, w9
100755c48:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c4c:      adds    w11, w9, w8
100755c50:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c54:      adds    w9, w25, w26
100755c58:      b.hs    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755c5c:      mov x0, #0x0                ; =0
100755c60:      adds    w9, w20, w9
100755c64:      cset    w10, hs
100755c68:      adds    w24, w9, w8
100755c6c:      cset    w8, hs
100755c70:      tbnz    w10, #0x0, 0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100755c74:      tbnz    w8, #0x0, 0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100755c78:      mov x20, x11
100755c7c:      add x23, x23, #0x1
100755c80:      ldr x8, [sp, #0x38]
100755c84:      cmp x23, x8
100755c88:      mov x22, x11
100755c8c:      mov x26, x24
100755c90:      mov w27, #0x28              ; =40
100755c94:      b.ne    0x10075594c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x24c>
100755c98:      adrp    x19, 0x101129000 <__MergedGlobals+0x38>
100755c9c:      add x19, x19, #0x768
100755ca0:      ldr x8, [x19]
100755ca4:      cmn x8, #0x1
100755ca8:      b.eq    0x100755cdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
100755cac:      mrs x9, TPIDRRO_EL0
100755cb0:      and x9, x9, #0xfffffffffffffff8
100755cb4:      ldr x8, [x9, x8, lsl #3]
100755cb8:      cbz x8, 0x100755cdc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
100755cbc:      ldr x8, [x8, #0x19e8]
100755cc0:      cbz x8, 0x100755d10 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x610>
100755cc4:      ldr x9, [x8]
100755cc8:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100755ccc:      cmp x9, x10
100755cd0:      b.hs    0x1007562b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbb0>
100755cd4:      ldr x21, [x8, #0x18]
100755cd8:      b   0x100755d20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x620>
100755cdc:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
100755ce0:      add x0, x0, #0x5f8
100755ce4:      ldr x8, [x0]
100755ce8:      blr x8
100755cec:      ldrb    w8, [x0, #0x20]
100755cf0:      cbnz    w8, 0x100756258 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb58>
100755cf4:      ldr x8, [x0]
100755cf8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100755cfc:      cmp x8, x9
100755d00:      ldr x20, [sp, #0x10]
100755d04:      b.hs    0x100756294 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb94>
100755d08:      ldr x21, [x0, #0x18]
100755d0c:      b   0x100755d24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x624>
100755d10:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100755d14:      add x0, x0, #0xb90
100755d18:      bl  0x1001354ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
100755d1c:      mov x21, x0
100755d20:      ldr x20, [sp, #0x10]
100755d24:      stur    x21, [x29, #-0x90]
100755d28:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
100755d2c:      stp x20, x8, [x29, #-0x78]
100755d30:      mov w8, #0x1                ; =1
100755d34:      stur    x8, [x29, #-0x80]
100755d38:      sub x0, x29, #0x80
100755d3c:      bl  0x10071175c <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100755d40:      mov x24, x0
100755d44:      stur    x0, [x29, #-0x88]
100755d48:      adrp    x0, 0x101130000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime8builtins7globals31STRUCTURED_CLONE_TRANSFER_STATE0023___RUST_STD_INTERNAL_VAL>
100755d4c:      add x0, x0, #0x258
100755d50:      ldr x8, [x0]
100755d54:      blr x8
100755d58:      strb    wzr, [x0]
100755d5c:      mov x0, x20
100755d60:      bl  0x1003e4824 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
100755d64:      tbz w0, #0x0, 0x100755e08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x708>
100755d68:      mov x0, x22
100755d6c:      bl  0x10071ea7c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
100755d70:      mov x23, x1
100755d74:      stp w26, w22, [x0]
100755d78:      stp wzr, wzr, [x0, #0xc]
100755d7c:      str w22, [x0, #0x8]
100755d80:      ldr x8, [x19]
100755d84:      cmn x8, #0x1
100755d88:      str x0, [sp, #0x20]
100755d8c:      b.eq    0x100755e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
100755d90:      mrs x9, TPIDRRO_EL0
100755d94:      and x9, x9, #0xfffffffffffffff8
100755d98:      ldr x8, [x9, x8, lsl #3]
100755d9c:      cbz x8, 0x100755e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
100755da0:      ldr x8, [x8, #0x19e8]
100755da4:      cbz x8, 0x100755f7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x87c>
100755da8:      ldr x9, [x8]
100755dac:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100755db0:      cmp x9, x10
100755db4:      b.hs    0x100756344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc44>
100755db8:      add x10, x9, #0x1
100755dbc:      str x10, [x8]
100755dc0:      ldr x10, [x8, #0x18]
100755dc4:      cmp x24, x10
100755dc8:      b.hs    0x100756254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
100755dcc:      ldr x10, [x8, #0x10]
100755dd0:      mov w11, #0x18              ; =24
100755dd4:      madd    x10, x24, x11, x10
100755dd8:      ldr x11, [x10]
100755ddc:      cmp x11, #0x1
100755de0:      b.ne    0x100756350 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc50>
100755de4:      ldr x22, [x10, #0x8]
100755de8:      str x9, [x8]
100755dec:      ldr w19, [x22, #0x4]
100755df0:      adrp    x8, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100755df4:      add x8, x8, #0x94
100755df8:      ldr w20, [x8]
100755dfc:      cmp w20, #0x300
100755e00:      b.lo    0x100755ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
100755e04:      b   0x100755ff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
100755e08:      ldr x8, [x19]
100755e0c:      cmn x8, #0x1
100755e10:      b.eq    0x100755f48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
100755e14:      mrs x9, TPIDRRO_EL0
100755e18:      and x9, x9, #0xfffffffffffffff8
100755e1c:      ldr x8, [x9, x8, lsl #3]
100755e20:      cbz x8, 0x100755f48 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
100755e24:      ldr x8, [x8, #0x19e8]
100755e28:      cbz x8, 0x100755fac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8ac>
100755e2c:      ldr x9, [x8]
100755e30:      cbnz    x9, 0x1007562bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
100755e34:      ldr x9, [x8, #0x18]
100755e38:      cmp x21, x9
100755e3c:      b.hi    0x100755e44 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x744>
100755e40:      str x21, [x8, #0x18]
100755e44:      str xzr, [x8]
100755e48:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755e4c:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
100755e50:      add x0, x0, #0x5f8
100755e54:      ldr x8, [x0]
100755e58:      blr x8
100755e5c:      ldrb    w8, [x0, #0x20]
100755e60:      cbnz    w8, 0x1007562c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbc8>
100755e64:      ldr x8, [x0]
100755e68:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100755e6c:      cmp x8, x9
100755e70:      b.hs    0x1007562f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbf8>
100755e74:      add x9, x8, #0x1
100755e78:      str x9, [x0]
100755e7c:      ldr x9, [x0, #0x18]
100755e80:      cmp x24, x9
100755e84:      b.hs    0x100756254 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
100755e88:      ldr x9, [x0, #0x10]
100755e8c:      mov w10, #0x18              ; =24
100755e90:      madd    x9, x24, x10, x9
100755e94:      ldr x10, [x9]
100755e98:      cmp x10, #0x1
100755e9c:      b.ne    0x1007562a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xba0>
100755ea0:      ldr x22, [x9, #0x8]
100755ea4:      str x8, [x0]
100755ea8:      ldr w19, [x22, #0x4]
100755eac:      adrp    x8, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100755eb0:      add x8, x8, #0x94
100755eb4:      ldr w20, [x8]
100755eb8:      cmp w20, #0x300
100755ebc:      b.hs    0x100755ff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
100755ec0:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
100755ec4:      add x8, x8, #0x768
100755ec8:      ldr x8, [x8]
100755ecc:      cmn x8, #0x1
100755ed0:      b.eq    0x100755fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
100755ed4:      mrs x9, TPIDRRO_EL0
100755ed8:      and x9, x9, #0xfffffffffffffff8
100755edc:      ldr x0, [x9, x8, lsl #3]
100755ee0:      cbz x0, 0x100755fe0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
100755ee4:      add x8, x0, x20, lsl #3
100755ee8:      ldr x0, [x8, #0x1e8]
100755eec:      cbz x0, 0x100755ff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
100755ef0:      ldr x0, [x0]
100755ef4:      cbz x0, 0x100756004 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x904>
100755ef8:      mov w8, #-0x40000001        ; =-1073741825
100755efc:      cmp w19, w8
100755f00:      str x21, [sp, #0x18]
100755f04:      b.gt    0x100756018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
100755f08:      ldr x9, [x0, #0x5198]
100755f0c:      ubfx    x8, x19, #15, #15
100755f10:      cmp x8, x9
100755f14:      b.hs    0x100756018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
100755f18:      ldr x9, [x0, #0x5190]
100755f1c:      ldr x8, [x9, x8, lsl #3]
100755f20:      cbz x8, 0x10075601c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
100755f24:      ubfx    x9, x19, #5, #10
100755f28:      ldr x8, [x8, x9, lsl #3]
100755f2c:      cbz x8, 0x10075601c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
100755f30:      and x9, x19, #0x1f
100755f34:      add x8, x8, x9, lsl #5
100755f38:      ldrb    w9, [x8, #0x1c]
100755f3c:      tbz w9, #0x0, 0x100756018 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
100755f40:      ldr x8, [x8]
100755f44:      b   0x10075601c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
100755f48:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
100755f4c:      add x0, x0, #0x5f8
100755f50:      ldr x8, [x0]
100755f54:      blr x8
100755f58:      ldrb    w8, [x0, #0x20]
100755f5c:      cbnz    w8, 0x100756304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc04>
100755f60:      ldr x8, [x0]
100755f64:      cbnz    x8, 0x100756380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
100755f68:      ldr x8, [x0, #0x18]
100755f6c:      cmp x21, x8
100755f70:      b.hi    0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755f74:      str x21, [x0, #0x18]
100755f78:      b   0x100755fbc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100755f7c:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100755f80:      add x0, x0, #0xb90
100755f84:      sub x1, x29, #0x88
100755f88:      bl  0x1001352d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
100755f8c:      mov x22, x0
100755f90:      ldr w19, [x0, #0x4]
100755f94:      adrp    x8, 0x10112a000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100755f98:      add x8, x8, #0x94
100755f9c:      ldr w20, [x8]
100755fa0:      cmp w20, #0x300
100755fa4:      b.lo    0x100755ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
100755fa8:      b   0x100755ff0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
100755fac:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100755fb0:      add x0, x0, #0xb90
100755fb4:      sub x1, x29, #0x90
100755fb8:      bl  0x100135888 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100755fbc:      mov x0, #0x0                ; =0
100755fc0:      add sp, sp, #0x5e0
100755fc4:      ldp x29, x30, [sp, #0x50]
100755fc8:      ldp x20, x19, [sp, #0x40]
100755fcc:      ldp x22, x21, [sp, #0x30]
100755fd0:      ldp x24, x23, [sp, #0x20]
100755fd4:      ldp x26, x25, [sp, #0x10]
100755fd8:      ldp x28, x27, [sp], #0x60
100755fdc:      ret
100755fe0:      bl  0x100cc8104 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100755fe4:      add x8, x0, x20, lsl #3
100755fe8:      ldr x0, [x8, #0x1e8]
100755fec:      cbnz    x0, 0x100755ef0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f0>
100755ff0:      adrp    x0, 0x1010d9000 <_anon.72cde5cdc14742b721629e115e16bf6f.1612+0x158>
100755ff4:      add x0, x0, #0x288
100755ff8:      bl  0x100cc7950 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
100755ffc:      ldr x0, [x0]
100756000:      cbnz    x0, 0x100755ef8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f8>
100756004:      bl  0x100cc7e88 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
100756008:      mov w8, #-0x40000001        ; =-1073741825
10075600c:      cmp w19, w8
100756010:      str x21, [sp, #0x18]
100756014:      b.le    0x100755f08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x808>
100756018:      mov x8, #0x0                ; =0
10075601c:      mov x19, #0x0               ; =0
100756020:      mov w9, #0x7b               ; =123
100756024:      strb    w9, [x23]
100756028:      add x26, x8, #0x8
10075602c:      add x25, x22, #0x10
100756030:      add x8, sp, #0x2c0
100756034:      add x8, x8, #0x28
100756038:      stp x8, x26, [sp, #0x28]
10075603c:      mov w21, #0x1               ; =1
100756040:      add x27, sp, #0x40
100756044:      mov w28, #0x3a              ; =58
100756048:      mov w20, #0x2c              ; =44
10075604c:      b   0x100756074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
100756050:      mov w8, #0x5d               ; =93
100756054:      strb    w8, [x23, x26]
100756058:      add x21, x26, #0x1
10075605c:      ldp x26, x8, [sp, #0x30]
100756060:      add x27, sp, #0x40
100756064:      mov w28, #0x3a              ; =58
100756068:      add x19, x19, #0x1
10075606c:      cmp x19, x8
100756070:      b.eq    0x1007561a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
100756074:      cbz x19, 0x100756080 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x980>
100756078:      strb    w20, [x23, x21]
10075607c:      add x21, x21, #0x1
100756080:      add x8, x19, x19, lsl #2
100756084:      lsl x24, x8, #3
100756088:      ldr x1, [x26, x19, lsl #3]
10075608c:      add x0, x27, x24
100756090:      add x2, x23, x21
100756094:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100756098:      add x8, x0, x21
10075609c:      strb    w28, [x23, x8]
1007560a0:      add x22, x8, #0x1
1007560a4:      ldr x1, [x25, x19, lsl #3]
1007560a8:      add x9, sp, #0x180
1007560ac:      add x0, x9, x24
1007560b0:      ldr w9, [x0]
1007560b4:      cmn w9, #0x1
1007560b8:      b.eq    0x1007560dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x9dc>
1007560bc:      add x2, x23, x22
1007560c0:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1007560c4:      add x21, x0, x22
1007560c8:      add x19, x19, #0x1
1007560cc:      ldr x8, [sp, #0x38]
1007560d0:      cmp x19, x8
1007560d4:      b.ne    0x100756074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
1007560d8:      b   0x1007561a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
1007560dc:      ldp x24, x21, [x0, #0x8]
1007560e0:      add x26, x8, #0x2
1007560e4:      mov w8, #0x5b               ; =91
1007560e8:      strb    w8, [x23, x22]
1007560ec:      cbz x21, 0x100756050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1007560f0:      cmp x24, #0xf
1007560f4:      b.hi    0x10075638c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc8c>
1007560f8:      and x22, x1, #0xffffffffffff
1007560fc:      add x8, sp, #0x2c0
100756100:      mov w9, #0x28               ; =40
100756104:      madd    x8, x24, x9, x8
100756108:      ldp q0, q1, [x8]
10075610c:      stp q0, q1, [x29, #-0x80]
100756110:      ldr x8, [x8, #0x20]
100756114:      stur    x8, [x29, #-0x60]
100756118:      ldr x1, [x22, #0x8]
10075611c:      sub x0, x29, #0x80
100756120:      add x2, x23, x26
100756124:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100756128:      add x26, x0, x26
10075612c:      subs    x28, x21, #0x1
100756130:      b.eq    0x100756050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
100756134:      add x21, x22, #0x10
100756138:      cmp x24, #0x10
10075613c:      mov w8, #0x10               ; =16
100756140:      csel    x8, x24, x8, lo
100756144:      mov w9, #0x28               ; =40
100756148:      sub x27, x8, #0xf
10075614c:      add x22, x24, #0x1
100756150:      ldr x8, [sp, #0x28]
100756154:      madd    x24, x24, x9, x8
100756158:      strb    w20, [x23, x26]
10075615c:      cbz x27, 0x100756390 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc90>
100756160:      add x26, x26, #0x1
100756164:      ldp q0, q1, [x24]
100756168:      stp q0, q1, [x29, #-0x80]
10075616c:      ldr x8, [x24, #0x20]
100756170:      stur    x8, [x29, #-0x60]
100756174:      ldr x1, [x21], #0x8
100756178:      sub x0, x29, #0x80
10075617c:      add x2, x23, x26
100756180:      bl  0x10074efe4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100756184:      add x26, x0, x26
100756188:      add x27, x27, #0x1
10075618c:      add x22, x22, #0x1
100756190:      add x24, x24, #0x28
100756194:      subs    x28, x28, #0x1
100756198:      b.ne    0x100756158 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xa58>
10075619c:      b   0x100756050 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1007561a0:      mov w8, #0x7d               ; =125
1007561a4:      strb    w8, [x23, x21]
1007561a8:      adrp    x8, 0x101129000 <__MergedGlobals+0x38>
1007561ac:      add x8, x8, #0x768
1007561b0:      ldr x8, [x8]
1007561b4:      cmn x8, #0x1
1007561b8:      b.eq    0x1007561f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
1007561bc:      mrs x9, TPIDRRO_EL0
1007561c0:      and x9, x9, #0xfffffffffffffff8
1007561c4:      ldr x8, [x9, x8, lsl #3]
1007561c8:      cbz x8, 0x1007561f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
1007561cc:      ldr x8, [x8, #0x19e8]
1007561d0:      ldr x10, [sp, #0x18]
1007561d4:      cbz x8, 0x100756230 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb30>
1007561d8:      ldr x9, [x8]
1007561dc:      cbnz    x9, 0x1007562bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
1007561e0:      ldr x9, [x8, #0x18]
1007561e4:      cmp x10, x9
1007561e8:      b.hi    0x1007561f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf0>
1007561ec:      str x10, [x8, #0x18]
1007561f0:      str xzr, [x8]
1007561f4:      b   0x100756240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1007561f8:      adrp    x0, 0x10112f000 <__RNvNCNKNvNtCs5gMwpk3Cs4e_13perry_runtime10event_pump11SPIN_STREAK0s_023___RUST_STD_INTERNAL_VAL+0x8>
1007561fc:      add x0, x0, #0x5f8
100756200:      ldr x8, [x0]
100756204:      blr x8
100756208:      ldrb    w8, [x0, #0x20]
10075620c:      ldr x20, [sp, #0x18]
100756210:      cbnz    w8, 0x100756330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc30>
100756214:      ldr x8, [x0]
100756218:      cbnz    x8, 0x100756380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
10075621c:      ldr x8, [x0, #0x18]
100756220:      cmp x20, x8
100756224:      b.hi    0x100756240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
100756228:      str x20, [x0, #0x18]
10075622c:      b   0x100756240 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
100756230:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100756234:      add x0, x0, #0xb90
100756238:      sub x1, x29, #0x90
10075623c:      bl  0x100135888 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100756240:      mov x1, #0x7fff000000000000 ; =9223090561878065152
100756244:      ldr x8, [sp, #0x20]
100756248:      bfxil   x1, x8, #0, #48
10075624c:      mov w0, #0x1                ; =1
100756250:      b   0x100755fc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100756254:      bl  0x100cb7368 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100756258:      cmp w8, #0x1
10075625c:      b.ne    0x100756338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
100756260:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100756264:      add x1, x1, #0x850
100756268:      mov x21, x0
10075626c:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100756270:      mov x0, x21
100756274:      strb    wzr, [x21, #0x20]
100756278:      mov x22, x20
10075627c:      mov x26, x24
100756280:      ldr x8, [x21]
100756284:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100756288:      cmp x8, x9
10075628c:      ldr x20, [sp, #0x10]
100756290:      b.lo    0x100755d08 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x608>
100756294:      adrp    x0, 0x101098000 <_anon.68a532d94142320e15103d7866c451bd.21>
100756298:      add x0, x0, #0x468
10075629c:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1007562a0:      adrp    x0, 0x100dba000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x26e0>
1007562a4:      add x0, x0, #0x7b0
1007562a8:      mov w1, #0xb                ; =11
1007562ac:      bl  0x100cb7330 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
1007562b0:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
1007562b4:      add x0, x0, #0xd08
1007562b8:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1007562bc:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
1007562c0:      add x0, x0, #0xe00
1007562c4:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
1007562c8:      cmp w8, #0x2
1007562cc:      b.eq    0x100756338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
1007562d0:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
1007562d4:      add x1, x1, #0x850
1007562d8:      mov x22, x0
1007562dc:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1007562e0:      mov x0, x22
1007562e4:      strb    wzr, [x22, #0x20]
1007562e8:      ldr x8, [x22]
1007562ec:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1007562f0:      cmp x8, x9
1007562f4:      b.lo    0x100755e74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x774>
1007562f8:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1007562fc:      add x0, x0, #0xf70
100756300:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100756304:      cmp w8, #0x2
100756308:      b.eq    0x100756338 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
10075630c:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100756310:      add x1, x1, #0x850
100756314:      mov x19, x0
100756318:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
10075631c:      mov x0, x19
100756320:      strb    wzr, [x19, #0x20]
100756324:      ldr x8, [x19]
100756328:      cbz x8, 0x100755f68 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x868>
10075632c:      b   0x100756380 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
100756330:      cmp w8, #0x2
100756334:      b.ne    0x100756360 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc60>
100756338:      adrp    x0, 0x101097000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
10075633c:      add x0, x0, #0xed8
100756340:      bl  0x100cd3f9c <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100756344:      adrp    x0, 0x1010be000 <_anon.4ff118d01ccdc9bd41517af7abf33093.966+0x540>
100756348:      add x0, x0, #0xc90
10075634c:      bl  0x100c8d25c <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100756350:      adrp    x0, 0x100dfd000 <_anon.4ff118d01ccdc9bd41517af7abf33093.1077+0xe2>
100756354:      add x0, x0, #0xa9a
100756358:      mov w1, #0xb                ; =11
10075635c:      bl  0x100cb7330 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100756360:      adrp    x1, 0x100a0f000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x518>
100756364:      add x1, x1, #0x850
100756368:      mov x19, x0
10075636c:      bl  0x100b9b3dc <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100756370:      mov x0, x19
100756374:      strb    wzr, [x19, #0x20]
100756378:      ldr x8, [x19]
10075637c:      cbz x8, 0x10075621c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb1c>
100756380:      adrp    x0, 0x10109d000 <_anon.68a532d94142320e15103d7866c451bd.1142>
100756384:      add x0, x0, #0x270
100756388:      bl  0x100c8d22c <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10075638c:      mov x22, x24
100756390:      adrp    x2, 0x1010bf000 <_anon.fd7e678389f6d6013308189123b84ec8.144+0x50>
100756394:      add x2, x2, #0x6a8
100756398:      mov x0, x22
10075639c:      mov w1, #0x10               ; =16
1007563a0:      bl  0x100c8d38c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
