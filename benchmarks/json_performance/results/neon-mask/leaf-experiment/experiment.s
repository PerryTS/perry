        .build_version macos, 11, 0
        .section        __TEXT,__text,regular,pure_instructions
        .private_extern __RINvNtCs8BpVhDwHqJW_3std2rt10lang_startuECsjWM3FVHqJkI_10experiment
        .globl  __RINvNtCs8BpVhDwHqJW_3std2rt10lang_startuECsjWM3FVHqJkI_10experiment
        .p2align        2
__RINvNtCs8BpVhDwHqJW_3std2rt10lang_startuECsjWM3FVHqJkI_10experiment:
        .cfi_startproc
        sub     sp, sp, #32
        .cfi_def_cfa_offset 32
        stp     x29, x30, [sp, #16]
        add     x29, sp, #16
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        mov     x4, x3
        mov     x3, x2
        mov     x2, x1
        str     x0, [sp, #8]
Lloh0:
        adrp    x1, l_anon.ffb3631f6cd35efadd1993fc3f34d432.0@PAGE
Lloh1:
        add     x1, x1, l_anon.ffb3631f6cd35efadd1993fc3f34d432.0@PAGEOFF
        add     x0, sp, #8
        bl      __RNvNtCs8BpVhDwHqJW_3std2rt19lang_start_internal
        .cfi_def_cfa wsp, 32
        ldp     x29, x30, [sp, #16]
        add     sp, sp, #32
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
        .loh AdrpAdd    Lloh0, Lloh1
        .cfi_endproc

        .p2align        2
__RINvNtNtCs8BpVhDwHqJW_3std3sys9backtrace28___rust_begin_short_backtraceFEuuECsjWM3FVHqJkI_10experiment:
        .cfi_startproc
        stp     x29, x30, [sp, #-16]!
        .cfi_def_cfa_offset 16
        mov     x29, sp
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        blr     x0
        ; InlineAsm Start
        ; InlineAsm End
        .cfi_def_cfa wsp, 16
        ldp     x29, x30, [sp], #16
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
        .cfi_endproc

        .p2align        2
__RNCINvNtCs8BpVhDwHqJW_3std2rt10lang_startuE0CsjWM3FVHqJkI_10experiment:
        .cfi_startproc
        stp     x29, x30, [sp, #-16]!
        .cfi_def_cfa_offset 16
        mov     x29, sp
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        ldr     x0, [x0]
        bl      __RINvNtNtCs8BpVhDwHqJW_3std3sys9backtrace28___rust_begin_short_backtraceFEuuECsjWM3FVHqJkI_10experiment
        mov     w0, #0
        .cfi_def_cfa wsp, 16
        ldp     x29, x30, [sp], #16
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
        .cfi_endproc

        .p2align        2
__RNSNvYNCINvNtCs8BpVhDwHqJW_3std2rt10lang_startuE0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceuE9call_once6vtableCsjWM3FVHqJkI_10experiment:
        .cfi_startproc
        stp     x29, x30, [sp, #-16]!
        .cfi_def_cfa_offset 16
        mov     x29, sp
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        ldr     x0, [x0]
        bl      __RINvNtNtCs8BpVhDwHqJW_3std3sys9backtrace28___rust_begin_short_backtraceFEuuECsjWM3FVHqJkI_10experiment
        mov     w0, #0
        .cfi_def_cfa wsp, 16
        ldp     x29, x30, [sp], #16
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
        .cfi_endproc

        .section        __TEXT,__literal16,16byte_literals
        .p2align        4, 0x0
lCPI4_0:
        .byte   34
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .byte   97
        .section        __TEXT,__text,regular,pure_instructions
        .private_extern __RNvCsjWM3FVHqJkI_10experiment4main
        .globl  __RNvCsjWM3FVHqJkI_10experiment4main
        .p2align        2
__RNvCsjWM3FVHqJkI_10experiment4main:
Lfunc_begin0:
        .cfi_startproc
        .cfi_personality 155, _rust_eh_personality
        .cfi_lsda 16, Lexception0
        sub     sp, sp, #208
        .cfi_def_cfa_offset 208
        stp     x28, x27, [sp, #112]
        stp     x26, x25, [sp, #128]
        stp     x24, x23, [sp, #144]
        stp     x22, x21, [sp, #160]
        stp     x20, x19, [sp, #176]
        stp     x29, x30, [sp, #192]
        add     x29, sp, #192
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        .cfi_offset w19, -24
        .cfi_offset w20, -32
        .cfi_offset w21, -40
        .cfi_offset w22, -48
        .cfi_offset w23, -56
        .cfi_offset w24, -64
        .cfi_offset w25, -72
        .cfi_offset w26, -80
        .cfi_offset w27, -88
        .cfi_offset w28, -96
        .cfi_remember_state
        mov     x22, #0
        mov     x24, #0
        mov     x21, #0
        str     xzr, [sp, #40]
        mov     w27, #1
        mov     x19, #-9187201950435737472
LBB4_1:
        cbz     x21, LBB4_314
        mov     x28, #0
        and     x9, x21, #0xfffffffffffffff0
        and     x20, x21, #0x7ffffffffffffff0
        and     x23, x21, #0xf
        and     x8, x21, #0xfffffffffffffff0
        str     x8, [sp, #16]
        sub     x25, x21, x9
LBB4_3:
        mov     w24, #0
        add     x8, x28, #1
        stp     x8, x28, [sp]
        b       LBB4_5
LBB4_4:
        ldr     x28, [sp, #8]
        add     x1, x28, x21
        ldr     x0, [sp, #32]
        mov     w2, #1
        bl      __RNvCscohsQztBDlz_7___rustc14___rust_dealloc
        cmp     w24, #255
        ldr     w24, [sp, #28]
        b.eq    LBB4_313
LBB4_5:
        mov     x26, x25
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     x0, x28, x21
        mov     w1, #1
        bl      __RNvCscohsQztBDlz_7___rustc12___rust_alloc
        cbz     x0, LBB4_434
        add     w8, w24, #1
        str     w8, [sp, #28]
        add     x2, x28, x21
        mov     w1, #97
        mov     x25, x0
        bl      _memset
        mov     x9, #0
        add     x8, x25, x28
        str     x25, [sp, #32]
        ldr     x10, [sp, #16]
        add     x10, x25, x10
Lloh2:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.10@PAGE
Lloh3:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.10@PAGEOFF
        mov     x4, #2025524839466146844
        orr     x4, x4, #0x4444444444444444
        mov     x6, #2314885530818453536
        movk    x6, #8223
Lloh4:
        adrp    x7, l_anon.ffb3631f6cd35efadd1993fc3f34d432.11@PAGE
Lloh5:
        add     x7, x7, l_anon.ffb3631f6cd35efadd1993fc3f34d432.11@PAGEOFF
Lloh6:
        adrp    x30, l_anon.ffb3631f6cd35efadd1993fc3f34d432.12@PAGE
Lloh7:
        add     x30, x30, l_anon.ffb3631f6cd35efadd1993fc3f34d432.12@PAGEOFF
        movi.16b        v3, #34
        movi.16b        v4, #92
        movi.16b        v5, #32
        movi.16b        v6, #237
        mov     w28, #97
        mov     x25, x26
        mov     x26, #72340172838076673
        movk    x26, #256
        b       LBB4_9
LBB4_7:
        str     xzr, [sp, #96]
        tbz     w14, #0, LBB4_426
LBB4_8:
        strb    w28, [x8, x11]
        add     x22, x22, #1
        cmp     x9, x21
        b.eq    LBB4_4
LBB4_9:
        mov     x12, #0
        mov     x11, x9
        add     x9, x9, #1
        strb    w24, [x8, x11]
LBB4_10:
        ldrb    w13, [x8, x12]
        cmp     w13, #34
        b.eq    LBB4_15
        cmp     w13, #92
        b.eq    LBB4_15
        cmp     w13, #32
        b.lo    LBB4_15
        add     x12, x12, #1
        cmp     x21, x12
        b.ne    LBB4_10
        mov     x13, #0
        mov     w14, #1
        stp     x13, x12, [sp, #48]
        cmp     x21, #16
        b.lo    LBB4_16
        b       LBB4_20
LBB4_15:
        mov     w14, #0
        mov     w13, #1
        stp     x13, x12, [sp, #48]
        cmp     x21, #16
        b.hs    LBB4_20
LBB4_16:
        cmp     x23, #8
        b.hs    LBB4_24
        mov     x0, #0
        cmp     x0, x23
        b.hi    LBB4_421
LBB4_18:
        orr     x15, x20, x0
        cmp     x15, x21
        b.ne    LBB4_35
LBB4_19:
        str     xzr, [sp, #96]
        tbnz    w14, #0, LBB4_84
        b       LBB4_427
LBB4_20:
        ldr     q0, [x8]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_32
        mov     x15, #0
LBB4_22:
        shl.16b v0, v0, #7
        cmlt.16b        v0, v0, #0
        fmov    x16, d0
        cbz     x16, LBB4_40
        rbit    x16, x16
        clz     x16, x16
        orr     x15, x15, x16, lsr #3
        b       LBB4_82
LBB4_24:
        ldr     x15, [x8, x20]
        eor     x16, x15, #0x2222222222222222
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_28
        eor     x16, x15, x4
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_28
        sub     x16, x6, x15
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_28
        mov     w0, #8
        cmp     x0, x23
        b.ls    LBB4_18
        b       LBB4_421
LBB4_28:
        mov     x0, #0
        and     w17, w15, #0xff
        mov     x16, #0
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_41
        mov     x0, #0
        mov     x16, #0
        b       LBB4_81
LBB4_32:
        cmp     x21, #32
        b.lo    LBB4_16
        ldr     q0, [x8, #16]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_45
        mov     w15, #16
        b       LBB4_22
LBB4_35:
        mov     x16, #0
        add     x15, x10, x0
        sub     x17, x25, x0
LBB4_36:
        ldrb    w1, [x15, x16]
        cmp     w1, #34
        b.eq    LBB4_81
        cmp     w1, #92
        b.eq    LBB4_81
        cmp     w1, #32
        b.lo    LBB4_81
        add     x16, x16, #1
        cmp     x17, x16
        b.ne    LBB4_36
        b       LBB4_19
LBB4_40:
        mov.d   x16, v0[1]
        rbit    x16, x16
        clz     x16, x16
        add     x15, x15, x16, lsr #3
        add     x15, x15, #8
        b       LBB4_82
LBB4_41:
        mov     x0, #0
        ubfx    w17, w15, #8, #8
        mov     w16, #1
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_48
        mov     x0, #0
        mov     w16, #1
        b       LBB4_81
LBB4_45:
        cmp     x21, #48
        b.lo    LBB4_16
        ldr     q0, [x8, #32]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_52
        mov     w15, #32
        b       LBB4_22
LBB4_48:
        mov     x0, #0
        ubfx    w17, w15, #16, #8
        mov     w16, #2
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_55
        mov     x0, #0
        mov     w16, #2
        b       LBB4_81
LBB4_52:
        cmp     x21, #64
        b.lo    LBB4_16
        ldr     q0, [x8, #48]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_59
        mov     w15, #48
        b       LBB4_22
LBB4_55:
        mov     x0, #0
        lsr     w17, w15, #24
        mov     w16, #3
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_65
        mov     x0, #0
        mov     w16, #3
        b       LBB4_81
LBB4_59:
        cmp     x21, #80
        b.lo    LBB4_16
        ldr     q0, [x8, #64]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_62
        mov     w15, #64
        b       LBB4_22
LBB4_62:
        cmp     x21, #96
        b.lo    LBB4_16
        ldr     q0, [x8, #80]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_16
        mov     w15, #80
        b       LBB4_22
LBB4_65:
        mov     x0, #0
        ubfx    x17, x15, #32, #8
        mov     w16, #4
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_69
        mov     x0, #0
        mov     w16, #4
        b       LBB4_81
LBB4_69:
        mov     x0, #0
        ubfx    x17, x15, #40, #8
        mov     w16, #5
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_73
        mov     x0, #0
        mov     w16, #5
        b       LBB4_81
LBB4_73:
        mov     x0, #0
        ubfx    x17, x15, #48, #8
        mov     w16, #6
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        cmp     w17, #32
        b.hs    LBB4_77
        mov     x0, #0
        mov     w16, #6
        b       LBB4_81
LBB4_77:
        mov     x0, #0
        lsr     x17, x15, #56
        mov     w16, #7
        cmp     w17, #34
        b.eq    LBB4_81
        cmp     w17, #92
        b.eq    LBB4_81
        lsr     x15, x15, #61
        cbnz    x15, LBB4_19
        mov     x0, #0
        mov     w16, #7
LBB4_81:
        add     x15, x16, x0
        add     x15, x15, x20
LBB4_82:
        stp     x27, x15, [sp, #96]
        add     x2, sp, #48
        add     x1, sp, #96
        cbz     x13, LBB4_428
        mov     x5, x3
        cmp     x15, x12
        b.ne    LBB4_429
LBB4_84:
        cmp     x21, #16
        b.hs    LBB4_89
LBB4_85:
        cmp     x23, #8
        b.hs    LBB4_93
        mov     x0, #0
        cmp     x0, x23
        b.hi    LBB4_424
LBB4_87:
        orr     x15, x20, x0
        cmp     x15, x21
        b.ne    LBB4_104
LBB4_88:
        str     xzr, [sp, #96]
        tbnz    w14, #0, LBB4_182
        b       LBB4_431
LBB4_89:
        ldr     q0, [x8]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_101
        mov     x15, #0
LBB4_91:
        shl.16b v0, v0, #7
        cmlt.16b        v0, v0, #0
        umov.b  w16, v0[0]
        cbz     w16, LBB4_109
        mov     x14, #0
        b       LBB4_180
LBB4_93:
        ldr     x15, [x8, x20]
        eor     x16, x15, #0x2222222222222222
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_97
        eor     x16, x15, x4
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_97
        sub     x16, x6, x15
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_97
        mov     w0, #8
        cmp     x0, x23
        b.ls    LBB4_87
        b       LBB4_424
LBB4_97:
        mov     x0, #0
        and     w17, w15, #0xff
        mov     x16, #0
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_113
        mov     x0, #0
        mov     x16, #0
        b       LBB4_175
LBB4_101:
        cmp     x21, #32
        b.lo    LBB4_85
        ldr     q0, [x8, #16]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_117
        mov     w15, #16
        b       LBB4_91
LBB4_104:
        mov     x16, #0
        add     x15, x10, x0
        sub     x17, x25, x0
LBB4_105:
        ldrb    w1, [x15, x16]
        cmp     w1, #34
        b.eq    LBB4_175
        cmp     w1, #92
        b.eq    LBB4_175
        cmp     w1, #32
        b.lo    LBB4_175
        add     x16, x16, #1
        cmp     x17, x16
        b.ne    LBB4_105
        b       LBB4_88
LBB4_109:
        umov.b  w16, v0[1]
        cbz     w16, LBB4_111
        mov     w14, #1
        b       LBB4_180
LBB4_111:
        umov.b  w16, v0[2]
        cbz     w16, LBB4_120
        mov     w14, #2
        b       LBB4_180
LBB4_113:
        mov     x0, #0
        ubfx    w17, w15, #8, #8
        mov     w16, #1
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_124
        mov     x0, #0
        mov     w16, #1
        b       LBB4_175
LBB4_117:
        cmp     x21, #48
        b.lo    LBB4_85
        ldr     q0, [x8, #32]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_128
        mov     w15, #32
        b       LBB4_91
LBB4_120:
        umov.b  w16, v0[3]
        cbz     w16, LBB4_122
        mov     w14, #3
        b       LBB4_180
LBB4_122:
        umov.b  w16, v0[4]
        cbz     w16, LBB4_131
        mov     w14, #4
        b       LBB4_180
LBB4_124:
        mov     x0, #0
        ubfx    w17, w15, #16, #8
        mov     w16, #2
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_133
        mov     x0, #0
        mov     w16, #2
        b       LBB4_175
LBB4_128:
        cmp     x21, #64
        b.lo    LBB4_85
        ldr     q0, [x8, #48]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_137
        mov     w15, #48
        b       LBB4_91
LBB4_131:
        umov.b  w16, v0[5]
        cbz     w16, LBB4_140
        mov     w14, #5
        b       LBB4_180
LBB4_133:
        mov     x0, #0
        lsr     w17, w15, #24
        mov     w16, #3
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_147
        mov     x0, #0
        mov     w16, #3
        b       LBB4_175
LBB4_137:
        cmp     x21, #80
        b.lo    LBB4_85
        ldr     q0, [x8, #64]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_144
        mov     w15, #64
        b       LBB4_91
LBB4_140:
        umov.b  w16, v0[6]
        cbz     w16, LBB4_142
        mov     w14, #6
        b       LBB4_180
LBB4_142:
        umov.b  w16, v0[7]
        cbz     w16, LBB4_151
        mov     w14, #7
        b       LBB4_180
LBB4_144:
        cmp     x21, #96
        b.lo    LBB4_85
        ldr     q0, [x8, #80]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v0, v5, v0
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_85
        mov     w15, #80
        b       LBB4_91
LBB4_147:
        mov     x0, #0
        ubfx    x17, x15, #32, #8
        mov     w16, #4
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_155
        mov     x0, #0
        mov     w16, #4
        b       LBB4_175
LBB4_151:
        umov.b  w16, v0[8]
        cbz     w16, LBB4_153
        mov     w14, #8
        b       LBB4_180
LBB4_153:
        umov.b  w16, v0[9]
        cbz     w16, LBB4_159
        mov     w14, #9
        b       LBB4_180
LBB4_155:
        mov     x0, #0
        ubfx    x17, x15, #40, #8
        mov     w16, #5
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_163
        mov     x0, #0
        mov     w16, #5
        b       LBB4_175
LBB4_159:
        umov.b  w16, v0[10]
        cbz     w16, LBB4_161
        mov     w14, #10
        b       LBB4_180
LBB4_161:
        umov.b  w16, v0[11]
        cbz     w16, LBB4_167
        mov     w14, #11
        b       LBB4_180
LBB4_163:
        mov     x0, #0
        ubfx    x17, x15, #48, #8
        mov     w16, #6
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        cmp     w17, #32
        b.hs    LBB4_171
        mov     x0, #0
        mov     w16, #6
        b       LBB4_175
LBB4_167:
        umov.b  w16, v0[12]
        cbz     w16, LBB4_169
        mov     w14, #12
        b       LBB4_180
LBB4_169:
        umov.b  w16, v0[13]
        cbz     w16, LBB4_176
        mov     w14, #13
        b       LBB4_180
LBB4_171:
        mov     x0, #0
        lsr     x17, x15, #56
        mov     w16, #7
        cmp     w17, #34
        b.eq    LBB4_175
        cmp     w17, #92
        b.eq    LBB4_175
        lsr     x15, x15, #61
        cbnz    x15, LBB4_88
        mov     x0, #0
        mov     w16, #7
LBB4_175:
        add     x14, x16, x0
        add     x14, x14, x20
        stp     x27, x14, [sp, #96]
        add     x2, sp, #48
        add     x1, sp, #96
        cbnz    x13, LBB4_181
        b       LBB4_423
LBB4_176:
        umov.b  w16, v0[14]
        cbz     w16, LBB4_178
        mov     w14, #14
        b       LBB4_180
LBB4_178:
        umov.b  w16, v0[15]
        cbz     w16, LBB4_88
        mov     w14, #15
LBB4_180:
        orr     x14, x14, x15
        stp     x27, x14, [sp, #96]
        add     x2, sp, #48
        add     x1, sp, #96
        cbz     x13, LBB4_423
LBB4_181:
        mov     x5, x7
        cmp     x14, x12
        b.ne    LBB4_429
LBB4_182:
        mov     x12, #0
LBB4_183:
        ldrb    w13, [x8, x12]
        cmp     w13, #34
        b.eq    LBB4_189
        cmp     w13, #92
        b.eq    LBB4_189
        cmp     w13, #32
        b.lo    LBB4_189
        cmp     w13, #237
        b.eq    LBB4_189
        add     x12, x12, #1
        cmp     x21, x12
        b.ne    LBB4_183
        mov     x13, #0
        mov     w14, #1
        stp     x13, x12, [sp, #64]
        cmp     x21, #16
        b.lo    LBB4_190
        b       LBB4_194
LBB4_189:
        mov     w14, #0
        mov     w13, #1
        stp     x13, x12, [sp, #64]
        cmp     x21, #16
        b.hs    LBB4_194
LBB4_190:
        cmp     x23, #8
        b.hs    LBB4_198
        mov     x0, #0
        cmp     x0, x23
        b.hi    LBB4_421
LBB4_192:
        orr     x15, x20, x0
        cmp     x15, x21
        b.ne    LBB4_208
LBB4_193:
        str     xzr, [sp, #96]
        tbnz    w14, #0, LBB4_216
        b       LBB4_432
LBB4_194:
        ldr     q0, [x8]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_205
        mov     x14, #0
LBB4_196:
        shl.16b v0, v0, #7
        cmlt.16b        v0, v0, #0
        fmov    x15, d0
        cbz     x15, LBB4_214
        rbit    x15, x15
        clz     x15, x15
        orr     x14, x14, x15, lsr #3
        stp     x27, x14, [sp, #96]
        add     x2, sp, #64
        add     x1, sp, #96
        cbnz    x13, LBB4_215
        b       LBB4_422
LBB4_198:
        ldr     x15, [x8, x20]
        eor     x16, x15, #0x2222222222222222
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_202
        eor     x16, x15, x4
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_202
        sub     x16, x6, x15
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_202
        mov     x16, #-3689348814741910324
        orr     x16, x16, #0xe1e1e1e1e1e1e1e1
        eor     x16, x15, x16
        mov     x17, #-72340172838076674
        movk    x17, #65279
        add     x16, x16, x17
        and     x16, x15, x16
        tst     x16, #0x8080808080808080
        b.eq    LBB4_270
LBB4_202:
        mov     x0, #0
        and     w17, w15, #0xff
        mov     x16, #0
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.ne    LBB4_256
LBB4_204:
        add     x14, x16, x0
        add     x14, x14, x20
        stp     x27, x14, [sp, #96]
        add     x2, sp, #64
        add     x1, sp, #96
        cbnz    x13, LBB4_215
        b       LBB4_422
LBB4_205:
        cmp     x21, #32
        b.lo    LBB4_190
        ldr     q0, [x8, #16]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_264
        mov     w14, #16
        b       LBB4_196
LBB4_208:
        mov     x16, #0
        add     x15, x10, x0
        sub     x17, x25, x0
LBB4_209:
        ldrb    w1, [x15, x16]
        cmp     w1, #34
        b.eq    LBB4_204
        cmp     w1, #92
        b.eq    LBB4_204
        cmp     w1, #32
        b.lo    LBB4_204
        cmp     w1, #237
        b.eq    LBB4_204
        add     x16, x16, #1
        cmp     x17, x16
        b.ne    LBB4_209
        b       LBB4_193
LBB4_214:
        mov.d   x15, v0[1]
        rbit    x15, x15
        clz     x15, x15
        add     x14, x14, x15, lsr #3
        add     x14, x14, #8
        stp     x27, x14, [sp, #96]
        add     x2, sp, #64
        add     x1, sp, #96
        cbz     x13, LBB4_422
LBB4_215:
        mov     x5, x30
        cmp     x14, x12
        b.ne    LBB4_429
LBB4_216:
        mov     x12, #0
LBB4_217:
        ldrb    w13, [x8, x12]
        cmp     w13, #34
        b.eq    LBB4_221
        cmp     w13, #92
        b.eq    LBB4_221
        add     x12, x12, #1
        cmp     x21, x12
        b.ne    LBB4_217
        mov     x13, #0
        mov     w14, #1
        stp     x13, x12, [sp, #80]
        cmp     x21, #16
        b.hs    LBB4_222
        b       LBB4_227
LBB4_221:
        mov     w14, #0
        mov     w13, #1
        stp     x13, x12, [sp, #80]
        cmp     x21, #16
        b.lo    LBB4_227
LBB4_222:
        ldr     q0, [x8]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_226
        mov     x14, #0
LBB4_224:
        shl.16b v0, v0, #7
        cmlt.16b        v0, v0, #0
        fmov    x15, d0
        cbz     x15, LBB4_254
        rbit    x15, x15
        clz     x15, x15
        orr     x14, x14, x15, lsr #3
        stp     x27, x14, [sp, #96]
        add     x2, sp, #80
        add     x1, sp, #96
Lloh8:
        adrp    x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGE
Lloh9:
        add     x5, x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGEOFF
        cbnz    x13, LBB4_255
        b       LBB4_429
LBB4_226:
        cmp     x21, #32
        b.hs    LBB4_262
LBB4_227:
        cmp     x23, #8
        b.hs    LBB4_234
        mov     x0, #0
        cmp     x0, x23
        b.hi    LBB4_421
LBB4_229:
        orr     x15, x20, x0
        cmp     x15, x21
        b.eq    LBB4_7
        mov     x16, #0
        sub     x15, x25, x0
        add     x17, x10, x0
LBB4_231:
        ldrb    w1, [x17, x16]
        cmp     w1, #92
        b.eq    LBB4_253
        cmp     w1, #34
        b.eq    LBB4_253
        add     x16, x16, #1
        cmp     x15, x16
        b.ne    LBB4_231
        b       LBB4_7
LBB4_234:
        ldr     x15, [x8, x20]
        eor     x16, x15, #0x2222222222222222
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_237
        eor     x16, x15, x4
        sub     x16, x26, x16
        orr     x16, x16, x15
        bics    xzr, x19, x16
        b.ne    LBB4_237
        mov     w0, #8
        cmp     x0, x23
        b.ls    LBB4_229
        b       LBB4_421
LBB4_237:
        mov     x0, #0
        and     w17, w15, #0xff
        mov     x16, #0
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        ubfx    w17, w15, #8, #8
        mov     w16, #1
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x16, x15, #16
        and     w17, w16, #0xff
        mov     w16, #2
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x16, x15, #24
        and     w17, w16, #0xff
        mov     w16, #3
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x16, x15, #32
        and     w17, w16, #0xff
        mov     w16, #4
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x16, x15, #40
        and     w17, w16, #0xff
        mov     w16, #5
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x16, x15, #48
        and     w17, w16, #0xff
        mov     w16, #6
        cmp     w17, #34
        b.eq    LBB4_253
        cmp     w17, #92
        b.eq    LBB4_253
        mov     x0, #0
        lsr     x15, x15, #56
        mov     w16, #7
        cmp     w15, #34
        b.eq    LBB4_253
        cmp     w15, #92
        b.ne    LBB4_7
LBB4_253:
        add     x14, x16, x0
        add     x14, x14, x20
        stp     x27, x14, [sp, #96]
        add     x2, sp, #80
        add     x1, sp, #96
Lloh10:
        adrp    x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGE
Lloh11:
        add     x5, x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGEOFF
        cbnz    x13, LBB4_255
        b       LBB4_429
LBB4_254:
        mov.d   x15, v0[1]
        rbit    x15, x15
        clz     x15, x15
        add     x14, x14, x15, lsr #3
        add     x14, x14, #8
        stp     x27, x14, [sp, #96]
        add     x2, sp, #80
        add     x1, sp, #96
Lloh12:
        adrp    x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGE
Lloh13:
        add     x5, x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGEOFF
        cbz     x13, LBB4_429
LBB4_255:
        cmp     x14, x12
        b.eq    LBB4_8
        b       LBB4_429
LBB4_256:
        mov     x0, #0
        mov     x16, #0
        cmp     w17, #32
        b.lo    LBB4_204
        cmp     w17, #237
        b.eq    LBB4_204
        mov     x0, #0
        ubfx    w17, w15, #8, #8
        mov     w16, #1
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_271
        mov     w16, #1
        b       LBB4_204
LBB4_262:
        ldr     q0, [x8, #16]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_267
        mov     w14, #16
        b       LBB4_224
LBB4_264:
        cmp     x21, #48
        b.lo    LBB4_190
        ldr     q0, [x8, #32]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_275
        mov     w14, #32
        b       LBB4_196
LBB4_267:
        cmp     x21, #48
        b.lo    LBB4_227
        ldr     q0, [x8, #32]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_278
        mov     w14, #32
        b       LBB4_224
LBB4_270:
        mov     w0, #8
        cmp     x0, x23
        b.ls    LBB4_192
        b       LBB4_421
LBB4_271:
        ubfx    w17, w15, #16, #8
        mov     w16, #2
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_281
        mov     w16, #2
        b       LBB4_204
LBB4_275:
        cmp     x21, #64
        b.lo    LBB4_190
        ldr     q0, [x8, #48]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_285
        mov     w14, #48
        b       LBB4_196
LBB4_278:
        cmp     x21, #64
        b.lo    LBB4_227
        ldr     q0, [x8, #48]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_288
        mov     w14, #48
        b       LBB4_224
LBB4_281:
        lsr     w17, w15, #24
        mov     w16, #3
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_297
        mov     w16, #3
        b       LBB4_204
LBB4_285:
        cmp     x21, #80
        b.lo    LBB4_190
        ldr     q0, [x8, #64]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_291
        mov     w14, #64
        b       LBB4_196
LBB4_288:
        cmp     x21, #80
        b.lo    LBB4_227
        ldr     q0, [x8, #64]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_294
        mov     w14, #64
        b       LBB4_224
LBB4_291:
        cmp     x21, #96
        b.lo    LBB4_190
        ldr     q0, [x8, #80]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v2, v0, v4
        orr.16b v1, v2, v1
        cmhi.16b        v2, v5, v0
        cmeq.16b        v0, v0, v6
        orr.16b v0, v0, v2
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_190
        mov     w14, #80
        b       LBB4_196
LBB4_294:
        cmp     x21, #96
        b.lo    LBB4_227
        ldr     q0, [x8, #80]
        cmeq.16b        v1, v0, v3
        cmeq.16b        v0, v0, v4
        orr.16b v0, v0, v1
        addp.2d d1, v0
        fmov    x15, d1
        cbz     x15, LBB4_227
        mov     w14, #80
        b       LBB4_224
LBB4_297:
        ubfx    x17, x15, #32, #8
        mov     w16, #4
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_301
        mov     w16, #4
        b       LBB4_204
LBB4_301:
        ubfx    x17, x15, #40, #8
        mov     w16, #5
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_305
        mov     w16, #5
        b       LBB4_204
LBB4_305:
        ubfx    x17, x15, #48, #8
        mov     w16, #6
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        cmp     w17, #237
        mov     w16, #32
        mov     x0, #0
        ccmp    w17, w16, #0, ne
        b.hs    LBB4_309
        mov     w16, #6
        b       LBB4_204
LBB4_309:
        lsr     x17, x15, #56
        mov     w16, #7
        cmp     w17, #34
        b.eq    LBB4_204
        cmp     w17, #92
        b.eq    LBB4_204
        lsr     x15, x15, #61
        cmp     x17, #237
        ccmp    x15, #0, #4, ne
        b.ne    LBB4_193
        mov     x0, #0
        mov     w16, #7
        b       LBB4_204
LBB4_313:
        ldr     x8, [sp, #16]
        add     x8, x8, #1
        str     x8, [sp, #16]
        mov     x24, x22
        ldr     x8, [sp]
        mov     x28, x8
        cmp     x8, #32
        b.ne    LBB4_3
        b       LBB4_376
LBB4_314:
        mov     w8, #0
LBB4_315:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_315
        mov     w8, #0
LBB4_317:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_317
        mov     w8, #0
LBB4_319:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_319
        mov     w8, #0
LBB4_321:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_321
        mov     w8, #0
LBB4_323:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_323
        mov     w8, #0
LBB4_325:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_325
        mov     w8, #0
LBB4_327:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_327
        mov     w8, #0
LBB4_329:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_329
        mov     w8, #0
LBB4_331:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_331
        mov     w8, #0
LBB4_333:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_333
        mov     w8, #0
LBB4_335:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_335
        mov     w8, #0
LBB4_337:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_337
        mov     w8, #0
LBB4_339:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_339
        mov     w8, #0
LBB4_341:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_341
        mov     w8, #0
LBB4_343:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_343
        mov     w8, #0
LBB4_345:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_345
        mov     w8, #0
LBB4_347:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_347
        mov     w8, #0
LBB4_349:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_349
        mov     w8, #0
LBB4_351:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_351
        mov     w8, #0
LBB4_353:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_353
        mov     w8, #0
LBB4_355:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_355
        mov     w8, #0
LBB4_357:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_357
        mov     w8, #0
LBB4_359:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_359
        mov     w8, #0
LBB4_361:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_361
        mov     w8, #0
LBB4_363:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_363
        mov     w8, #0
LBB4_365:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_365
        mov     w8, #0
LBB4_367:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_367
        mov     w8, #0
LBB4_369:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_369
        mov     w8, #0
LBB4_371:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_371
        mov     w8, #0
LBB4_373:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_373
        mov     w8, #0
LBB4_375:
        mov     x20, x8
        bl      __RNvCscohsQztBDlz_7___rustc35___rust_no_alloc_shim_is_unstable_v2
        add     w8, w20, #1
        cmp     w20, #255
        b.ne    LBB4_375
LBB4_376:
        add     x21, x21, #1
        cmp     x21, #97
        b.ne    LBB4_1
        mov     w8, #0
        str     x22, [sp, #40]
        movi.2d v0, #0000000000000000
Lloh14:
        adrp    x9, lCPI4_0@PAGE
Lloh15:
        ldr     q1, [x9, lCPI4_0@PAGEOFF]
        movi.16b        v2, #97
        mov     w9, #34
        movi.16b        v3, #34
        movi.16b        v4, #92
        movi.16b        v5, #32
        mov     w10, #1
LBB4_378:
        and     w11, w8, #0x1
        fmov    s6, w11
        cmeq.4s v6, v6, v0
        dup.4s  v6, v6[0]
        bsl.16b v6, v2, v1
        and     w11, w8, #0x2
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[1], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x4
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[2], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x8
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[3], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x10
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[4], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x20
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[5], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x40
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[6], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x80
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[7], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x100
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[8], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x200
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[9], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x400
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[10], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x800
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[11], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x1000
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[12], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x2000
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[13], w9
        bif.16b v6, v16, v7
        and     w11, w8, #0x4000
        fmov    s7, w11
        cmeq.4s v7, v7, v0
        dup.4s  v7, v7[0]
        mov.16b v16, v6
        mov.b   v16[14], w9
        and     w11, w8, #0x8000
        fmov    s17, w11
        cmeq.4s v17, v17, v0
        dup.4s  v17, v17[0]
        bif.16b v6, v16, v7
        mov.16b v7, v6
        mov.b   v7[15], w9
        bif.16b v6, v7, v17
        cmeq.16b        v7, v6, v3
        cmeq.16b        v16, v6, v4
        orr.16b v7, v16, v7
        cmhi.16b        v16, v5, v6
        orr.16b v7, v16, v7
        addp.2d d16, v7
        fmov    x11, d16
        cbz     x11, LBB4_381
        fmov    x11, d7
        cbz     x11, LBB4_382
        rbit    x11, x11
        clz     x11, x11
        lsr     x11, x11, #3
        b       LBB4_383
LBB4_381:
        mov     x12, #0
        b       LBB4_384
LBB4_382:
        mov.d   x11, v7[1]
        rbit    x11, x11
        clz     x11, x11
        lsr     x11, x11, #3
        add     x11, x11, #8
LBB4_383:
        mov     w12, #1
LBB4_384:
        umov.b  w13, v6[0]
        stp     x12, x11, [sp, #80]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_386
        mov     x13, #0
        b       LBB4_416
LBB4_386:
        umov.b  w13, v6[1]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_388
        mov     w13, #1
        b       LBB4_416
LBB4_388:
        umov.b  w13, v6[2]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_390
        mov     w13, #2
        b       LBB4_416
LBB4_390:
        umov.b  w13, v6[3]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_392
        mov     w13, #3
        b       LBB4_416
LBB4_392:
        umov.b  w13, v6[4]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_394
        mov     w13, #4
        b       LBB4_416
LBB4_394:
        umov.b  w13, v6[5]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_396
        mov     w13, #5
        b       LBB4_416
LBB4_396:
        umov.b  w13, v6[6]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_398
        mov     w13, #6
        b       LBB4_416
LBB4_398:
        umov.b  w13, v6[7]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_400
        mov     w13, #7
        b       LBB4_416
LBB4_400:
        umov.b  w13, v6[8]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_402
        mov     w13, #8
        b       LBB4_416
LBB4_402:
        umov.b  w13, v6[9]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_404
        mov     w13, #9
        b       LBB4_416
LBB4_404:
        umov.b  w13, v6[10]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_406
        mov     w13, #10
        b       LBB4_416
LBB4_406:
        umov.b  w13, v6[11]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_408
        mov     w13, #11
        b       LBB4_416
LBB4_408:
        umov.b  w13, v6[12]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_410
        mov     w13, #12
        b       LBB4_416
LBB4_410:
        umov.b  w13, v6[13]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_412
        mov     w13, #13
        b       LBB4_416
LBB4_412:
        umov.b  w13, v6[14]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_414
        mov     w13, #14
        b       LBB4_416
LBB4_414:
        umov.b  w13, v6[15]
        and     w13, w13, #0xff
        cmp     w13, #34
        b.ne    LBB4_419
        mov     w13, #15
LBB4_416:
        stp     x10, x13, [sp, #96]
        cbz     x12, LBB4_420
        cmp     x11, x13
        b.ne    LBB4_420
LBB4_418:
        add     w8, w8, #1
        cmp     w8, #16, lsl #12
        b.ne    LBB4_378
        b       LBB4_433
LBB4_419:
        str     xzr, [sp, #96]
        tbz     w12, #0, LBB4_418
LBB4_420:
Lloh16:
        adrp    x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.9@PAGE
Lloh17:
        add     x5, x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.9@PAGEOFF
        add     x1, sp, #80
        add     x2, sp, #96
        mov     w0, #0
        mov     x3, #0
        bl      __RINvNtCsjgY6bXVaRmE_4core9panicking13assert_failedINtNtB4_6option6OptionjEBM_EB4_
LBB4_421:
Lloh18:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.2@PAGE
Lloh19:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.2@PAGEOFF
        b       LBB4_425
LBB4_422:
        mov     x5, x30
        b       LBB4_429
LBB4_423:
        mov     x5, x7
        b       LBB4_429
LBB4_424:
Lloh20:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.5@PAGE
Lloh21:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.5@PAGEOFF
LBB4_425:
        str     x22, [sp, #40]
Ltmp2:
        and     x1, x21, #0xf
        and     x2, x21, #0xf
        ldr     x20, [sp, #8]
        bl      __RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail
Ltmp3:
        b       LBB4_430
LBB4_426:
        add     x2, sp, #80
        add     x1, sp, #96
Lloh22:
        adrp    x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGE
Lloh23:
        add     x5, x5, l_anon.ffb3631f6cd35efadd1993fc3f34d432.13@PAGEOFF
        b       LBB4_429
LBB4_427:
        add     x2, sp, #48
        add     x1, sp, #96
LBB4_428:
        mov     x5, x3
LBB4_429:
        str     x22, [sp, #40]
Ltmp0:
        mov     w0, #0
        mov     x3, #0
        ldr     x20, [sp, #8]
        bl      __RINvNtCsjgY6bXVaRmE_4core9panicking13assert_failedINtNtB4_6option6OptionjEBM_EB4_
Ltmp1:
LBB4_430:
        brk     #0x1
LBB4_431:
        add     x2, sp, #48
        add     x1, sp, #96
        mov     x5, x7
        b       LBB4_429
LBB4_432:
        add     x2, sp, #64
        add     x1, sp, #96
        mov     x5, x30
        b       LBB4_429
LBB4_433:
        add     x8, x24, #16, lsl #12
        str     x8, [sp, #40]
        add     x8, sp, #40
Lloh24:
        adrp    x9, __RNvXsi_NtNtNtCsjgY6bXVaRmE_4core3fmt3num3impjNtB9_7Display3fmt@GOTPAGE
Lloh25:
        ldr     x9, [x9, __RNvXsi_NtNtNtCsjgY6bXVaRmE_4core3fmt3num3impjNtB9_7Display3fmt@GOTPAGEOFF]
        stp     x8, x9, [sp, #96]
Lloh26:
        adrp    x0, l_anon.ffb3631f6cd35efadd1993fc3f34d432.7@PAGE
Lloh27:
        add     x0, x0, l_anon.ffb3631f6cd35efadd1993fc3f34d432.7@PAGEOFF
        add     x1, sp, #96
        bl      __RNvNtNtCs8BpVhDwHqJW_3std2io5stdio6__print
        .cfi_def_cfa wsp, 208
        ldp     x29, x30, [sp, #192]
        ldp     x20, x19, [sp, #176]
        ldp     x22, x21, [sp, #160]
        ldp     x24, x23, [sp, #144]
        ldp     x26, x25, [sp, #128]
        ldp     x28, x27, [sp, #112]
        add     sp, sp, #208
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        .cfi_restore w19
        .cfi_restore w20
        .cfi_restore w21
        .cfi_restore w22
        .cfi_restore w23
        .cfi_restore w24
        .cfi_restore w25
        .cfi_restore w26
        .cfi_restore w27
        .cfi_restore w28
        ret
LBB4_434:
        .cfi_restore_state
        str     x22, [sp, #40]
        add     x1, x28, x21
        mov     w0, #1
        bl      __RNvNtCsctvjasLqLe9_5alloc7raw_vec12handle_error
LBB4_435:
Ltmp4:
        mov     x19, x0
        add     x1, x20, x21
        ldr     x0, [sp, #32]
        mov     w2, #1
        bl      __RNvCscohsQztBDlz_7___rustc14___rust_dealloc
        mov     x0, x19
        bl      __Unwind_Resume
        .loh AdrpAdd    Lloh6, Lloh7
        .loh AdrpAdd    Lloh4, Lloh5
        .loh AdrpAdd    Lloh2, Lloh3
        .loh AdrpAdd    Lloh8, Lloh9
        .loh AdrpAdd    Lloh10, Lloh11
        .loh AdrpAdd    Lloh12, Lloh13
        .loh AdrpLdr    Lloh14, Lloh15
        .loh AdrpAdd    Lloh16, Lloh17
        .loh AdrpAdd    Lloh18, Lloh19
        .loh AdrpAdd    Lloh20, Lloh21
        .loh AdrpAdd    Lloh22, Lloh23
        .loh AdrpAdd    Lloh26, Lloh27
        .loh AdrpLdrGot Lloh24, Lloh25
Lfunc_end0:
        .cfi_endproc
        .section        __TEXT,__gcc_except_tab
        .p2align        2, 0x0
GCC_except_table4:
Lexception0:
        .byte   255
        .byte   255
        .byte   1
        .uleb128 Lcst_end0-Lcst_begin0
Lcst_begin0:
        .uleb128 Lfunc_begin0-Lfunc_begin0
        .uleb128 Ltmp2-Lfunc_begin0
        .byte   0
        .byte   0
        .uleb128 Ltmp2-Lfunc_begin0
        .uleb128 Ltmp1-Ltmp2
        .uleb128 Ltmp4-Lfunc_begin0
        .byte   0
        .uleb128 Ltmp1-Lfunc_begin0
        .uleb128 Lfunc_end0-Ltmp1
        .byte   0
        .byte   0
Lcst_end0:
        .p2align        2, 0x0

        .section        __TEXT,__text,regular,pure_instructions
        .globl  _new_scan
        .p2align        2
_new_scan:
Lfunc_begin1:
        .cfi_startproc
        .cfi_personality 155, _rust_eh_personality
        .cfi_lsda 16, Lexception1
        stp     x29, x30, [sp, #-16]!
        .cfi_def_cfa_offset 16
        mov     x29, sp
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        .cfi_remember_state
        mov     x8, x0
        mov     x9, #0
        movi.16b        v0, #34
        movi.16b        v1, #92
        movi.16b        v2, #32
LBB5_1:
        add     x10, x9, #16
        cmp     x10, x1
        b.hi    LBB5_5
        ldr     q3, [x8, x9]
        cmeq.16b        v4, v3, v0
        cmeq.16b        v5, v3, v1
        orr.16b v4, v5, v4
        cmhi.16b        v3, v2, v3
        orr.16b v3, v3, v4
        addp.2d d4, v3
        fmov    x11, d4
        mov     x9, x10
        cbz     x11, LBB5_1
        fmov    x8, d3
        cbz     x8, LBB5_15
        rbit    x8, x8
        clz     x8, x8
        add     x8, x10, x8, lsr #3
        sub     x1, x8, #16
        b       LBB5_57
LBB5_5:
        mov     x12, #0
        mov     x11, #72340172838076673
        movk    x11, #256
        sub     x2, x1, x9
        mov     x13, #-9187201950435737472
        mov     x14, #2025524839466146844
        orr     x14, x14, #0x4444444444444444
        mov     x15, #2314885530818453536
        movk    x15, #8223
LBB5_6:
        mov     x0, x12
        add     x12, x12, #8
        cmp     x12, x2
        b.hi    LBB5_16
        add     x10, x8, x0
        ldr     x10, [x10, x9]
        eor     x16, x10, #0x2222222222222222
        sub     x16, x11, x16
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.ne    LBB5_10
        eor     x16, x10, x14
        sub     x16, x11, x16
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.ne    LBB5_10
        sub     x16, x15, x10
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.eq    LBB5_6
LBB5_10:
        cmn     x0, #8
        b.eq    LBB5_40
        mov     x8, #0
        and     w11, w10, #0xff
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_24
        mov     x8, #0
        b       LBB5_56
LBB5_15:
        mov.d   x8, v3[1]
        rbit    x8, x8
        clz     x8, x8
        add     x8, x10, x8, lsr #3
        sub     x1, x8, #8
        b       LBB5_57
LBB5_16:
        cmp     x0, x2
        b.hi    LBB5_41
        sub     x10, x9, x1
        cmn     x10, x0
        b.eq    LBB5_57
        add     x9, x9, x0
LBB5_19:
        ldrb    w10, [x8, x9]
        cmp     w10, #34
        b.eq    LBB5_23
        cmp     w10, #92
        b.eq    LBB5_23
        cmp     w10, #32
        b.lo    LBB5_23
        add     x9, x9, #1
        cmp     x1, x9
        b.ne    LBB5_19
        b       LBB5_57
LBB5_23:
        mov     x1, x9
        b       LBB5_57
LBB5_24:
        ubfx    w11, w10, #8, #8
        mov     w8, #1
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_28
        mov     w8, #1
        b       LBB5_56
LBB5_28:
        ubfx    w11, w10, #16, #8
        mov     w8, #2
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_32
        mov     w8, #2
        b       LBB5_56
LBB5_32:
        lsr     w11, w10, #24
        mov     w8, #3
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_36
        mov     w8, #3
        b       LBB5_56
LBB5_36:
        ubfx    x11, x10, #32, #8
        mov     w8, #4
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_44
        mov     w8, #4
        b       LBB5_56
LBB5_40:
        add     x1, x0, #8
Lloh28:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.3@PAGE
Lloh29:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.3@PAGEOFF
        b       LBB5_42
LBB5_41:
Lloh30:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.2@PAGE
Lloh31:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.2@PAGEOFF
        mov     x1, x2
LBB5_42:
Ltmp5:
        bl      __RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail
Ltmp6:
        brk     #0x1
LBB5_44:
        ubfx    x11, x10, #40, #8
        mov     w8, #5
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_48
        mov     w8, #5
        b       LBB5_56
LBB5_48:
        ubfx    x11, x10, #48, #8
        mov     w8, #6
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        cmp     w11, #32
        b.hs    LBB5_52
        mov     w8, #6
        b       LBB5_56
LBB5_52:
        lsr     x11, x10, #56
        mov     w8, #7
        cmp     w11, #34
        b.eq    LBB5_56
        cmp     w11, #92
        b.eq    LBB5_56
        lsr     x8, x10, #61
        cbnz    x8, LBB5_57
        mov     w8, #7
LBB5_56:
        add     x8, x8, x0
        add     x1, x8, x9
LBB5_57:
        mov     x0, x1
        .cfi_def_cfa wsp, 16
        ldp     x29, x30, [sp], #16
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
LBB5_58:
        .cfi_restore_state
Ltmp7:
        bl      __RNvNtCsjgY6bXVaRmE_4core9panicking19panic_cannot_unwind
        .loh AdrpAdd    Lloh28, Lloh29
        .loh AdrpAdd    Lloh30, Lloh31
Lfunc_end1:
        .cfi_endproc
        .section        __TEXT,__gcc_except_tab
        .p2align        2, 0x0
GCC_except_table5:
Lexception1:
        .byte   255
        .byte   155
        .uleb128 Lttbase0-Lttbaseref0
Lttbaseref0:
        .byte   1
        .uleb128 Lcst_end1-Lcst_begin1
Lcst_begin1:
        .uleb128 Ltmp5-Lfunc_begin1
        .uleb128 Ltmp6-Ltmp5
        .uleb128 Ltmp7-Lfunc_begin1
        .byte   1
Lcst_end1:
        .byte   127
        .byte   0
        .p2align        2, 0x0
Lttbase0:
        .byte   0
        .p2align        2, 0x0

        .section        __TEXT,__text,regular,pure_instructions
        .globl  _old_scan
        .p2align        2
_old_scan:
Lfunc_begin2:
        .cfi_startproc
        .cfi_personality 155, _rust_eh_personality
        .cfi_lsda 16, Lexception2
        stp     x29, x30, [sp, #-16]!
        .cfi_def_cfa_offset 16
        mov     x29, sp
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        .cfi_remember_state
        mov     x8, x0
        mov     x9, #0
        movi.16b        v1, #34
        movi.16b        v2, #92
        movi.16b        v3, #32
LBB6_1:
        add     x10, x9, #16
        cmp     x10, x1
        b.hi    LBB6_5
        ldr     q0, [x8, x9]
        cmeq.16b        v4, v0, v1
        cmeq.16b        v5, v0, v2
        orr.16b v4, v5, v4
        cmhi.16b        v0, v3, v0
        orr.16b v0, v0, v4
        addp.2d d4, v0
        fmov    x11, d4
        mov     x9, x10
        cbz     x11, LBB6_1
        umov.b  w8, v0[0]
        cbz     w8, LBB6_15
        mov     x8, #0
        b       LBB6_86
LBB6_5:
        mov     x12, #0
        mov     x11, #72340172838076673
        movk    x11, #256
        sub     x2, x1, x9
        mov     x13, #-9187201950435737472
        mov     x14, #2025524839466146844
        orr     x14, x14, #0x4444444444444444
        mov     x15, #2314885530818453536
        movk    x15, #8223
LBB6_6:
        mov     x0, x12
        add     x12, x12, #8
        cmp     x12, x2
        b.hi    LBB6_17
        add     x10, x8, x0
        ldr     x10, [x10, x9]
        eor     x16, x10, #0x2222222222222222
        sub     x16, x11, x16
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.ne    LBB6_10
        eor     x16, x10, x14
        sub     x16, x11, x16
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.ne    LBB6_10
        sub     x16, x15, x10
        orr     x16, x16, x10
        bics    xzr, x13, x16
        b.eq    LBB6_6
LBB6_10:
        cmn     x0, #8
        b.eq    LBB6_55
        mov     x8, #0
        and     w11, w10, #0xff
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_27
        mov     x8, #0
        b       LBB6_81
LBB6_15:
        umov.b  w8, v0[1]
        cbz     w8, LBB6_24
        mov     w8, #1
        b       LBB6_86
LBB6_17:
        cmp     x0, x2
        b.hi    LBB6_58
        sub     x10, x9, x1
        cmn     x10, x0
        b.eq    LBB6_87
        add     x9, x9, x0
LBB6_20:
        ldrb    w10, [x8, x9]
        cmp     w10, #34
        b.eq    LBB6_26
        cmp     w10, #92
        b.eq    LBB6_26
        cmp     w10, #32
        b.lo    LBB6_26
        add     x9, x9, #1
        cmp     x1, x9
        b.ne    LBB6_20
        b       LBB6_87
LBB6_24:
        umov.b  w8, v0[2]
        cbz     w8, LBB6_31
        mov     w8, #2
        b       LBB6_86
LBB6_26:
        mov     x1, x9
        b       LBB6_87
LBB6_27:
        ubfx    w11, w10, #8, #8
        mov     w8, #1
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_33
        mov     w8, #1
        b       LBB6_81
LBB6_31:
        umov.b  w8, v0[3]
        cbz     w8, LBB6_37
        mov     w8, #3
        b       LBB6_86
LBB6_33:
        ubfx    w11, w10, #16, #8
        mov     w8, #2
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_41
        mov     w8, #2
        b       LBB6_81
LBB6_37:
        umov.b  w8, v0[4]
        cbz     w8, LBB6_39
        mov     w8, #4
        b       LBB6_86
LBB6_39:
        umov.b  w8, v0[5]
        cbz     w8, LBB6_45
        mov     w8, #5
        b       LBB6_86
LBB6_41:
        lsr     w11, w10, #24
        mov     w8, #3
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_49
        mov     w8, #3
        b       LBB6_81
LBB6_45:
        umov.b  w8, v0[6]
        cbz     w8, LBB6_47
        mov     w8, #6
        b       LBB6_86
LBB6_47:
        umov.b  w8, v0[7]
        cbz     w8, LBB6_53
        mov     w8, #7
        b       LBB6_86
LBB6_49:
        ubfx    x11, x10, #32, #8
        mov     w8, #4
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_61
        mov     w8, #4
        b       LBB6_81
LBB6_53:
        umov.b  w8, v0[8]
        cbz     w8, LBB6_56
        mov     w8, #8
        b       LBB6_86
LBB6_55:
        add     x1, x0, #8
Lloh32:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.6@PAGE
Lloh33:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.6@PAGEOFF
        b       LBB6_59
LBB6_56:
        umov.b  w8, v0[9]
        cbz     w8, LBB6_65
        mov     w8, #9
        b       LBB6_86
LBB6_58:
Lloh34:
        adrp    x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.5@PAGE
Lloh35:
        add     x3, x3, l_anon.ffb3631f6cd35efadd1993fc3f34d432.5@PAGEOFF
        mov     x1, x2
LBB6_59:
Ltmp8:
        bl      __RNvNtNtCsjgY6bXVaRmE_4core5slice5index16slice_index_fail
Ltmp9:
        brk     #0x1
LBB6_61:
        ubfx    x11, x10, #40, #8
        mov     w8, #5
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_69
        mov     w8, #5
        b       LBB6_81
LBB6_65:
        umov.b  w8, v0[10]
        cbz     w8, LBB6_67
        mov     w8, #10
        b       LBB6_86
LBB6_67:
        umov.b  w8, v0[11]
        cbz     w8, LBB6_73
        mov     w8, #11
        b       LBB6_86
LBB6_69:
        ubfx    x11, x10, #48, #8
        mov     w8, #6
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        cmp     w11, #32
        b.hs    LBB6_77
        mov     w8, #6
        b       LBB6_81
LBB6_73:
        umov.b  w8, v0[12]
        cbz     w8, LBB6_75
        mov     w8, #12
        b       LBB6_86
LBB6_75:
        umov.b  w8, v0[13]
        cbz     w8, LBB6_82
        mov     w8, #13
        b       LBB6_86
LBB6_77:
        lsr     x11, x10, #56
        mov     w8, #7
        cmp     w11, #34
        b.eq    LBB6_81
        cmp     w11, #92
        b.eq    LBB6_81
        lsr     x8, x10, #61
        cbnz    x8, LBB6_87
        mov     w8, #7
LBB6_81:
        add     x8, x8, x0
        add     x1, x8, x9
        b       LBB6_87
LBB6_82:
        umov.b  w8, v0[14]
        cbz     w8, LBB6_84
        mov     w8, #14
        b       LBB6_86
LBB6_84:
        umov.b  w8, v0[15]
        cbz     w8, LBB6_87
        mov     w8, #15
LBB6_86:
        add     x8, x8, x10
        sub     x1, x8, #16
LBB6_87:
        mov     x0, x1
        .cfi_def_cfa wsp, 16
        ldp     x29, x30, [sp], #16
        .cfi_def_cfa_offset 0
        .cfi_restore w30
        .cfi_restore w29
        ret
LBB6_88:
        .cfi_restore_state
Ltmp10:
        bl      __RNvNtCsjgY6bXVaRmE_4core9panicking19panic_cannot_unwind
        .loh AdrpAdd    Lloh32, Lloh33
        .loh AdrpAdd    Lloh34, Lloh35
Lfunc_end2:
        .cfi_endproc
        .section        __TEXT,__gcc_except_tab
        .p2align        2, 0x0
GCC_except_table6:
Lexception2:
        .byte   255
        .byte   155
        .uleb128 Lttbase1-Lttbaseref1
Lttbaseref1:
        .byte   1
        .uleb128 Lcst_end2-Lcst_begin2
Lcst_begin2:
        .uleb128 Ltmp8-Lfunc_begin2
        .uleb128 Ltmp9-Ltmp8
        .uleb128 Ltmp10-Lfunc_begin2
        .byte   1
Lcst_end2:
        .byte   127
        .byte   0
        .p2align        2, 0x0
Lttbase1:
        .byte   0
        .p2align        2, 0x0

        .section        __TEXT,__text,regular,pure_instructions
        .globl  _main
        .p2align        2
_main:
        .cfi_startproc
        sub     sp, sp, #32
        stp     x29, x30, [sp, #16]
        add     x29, sp, #16
        .cfi_def_cfa w29, 16
        .cfi_offset w30, -8
        .cfi_offset w29, -16
        mov     x3, x1
        sxtw    x2, w0
Lloh36:
        adrp    x8, __RNvCsjWM3FVHqJkI_10experiment4main@PAGE
Lloh37:
        add     x8, x8, __RNvCsjWM3FVHqJkI_10experiment4main@PAGEOFF
        str     x8, [sp, #8]
Lloh38:
        adrp    x1, l_anon.ffb3631f6cd35efadd1993fc3f34d432.0@PAGE
Lloh39:
        add     x1, x1, l_anon.ffb3631f6cd35efadd1993fc3f34d432.0@PAGEOFF
        add     x0, sp, #8
        mov     w4, #0
        bl      __RNvNtCs8BpVhDwHqJW_3std2rt19lang_start_internal
        ldp     x29, x30, [sp, #16]
        add     sp, sp, #32
        ret
        .loh AdrpAdd    Lloh38, Lloh39
        .loh AdrpAdd    Lloh36, Lloh37
        .cfi_endproc

        .section        __DATA,__const
        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.0:
        .asciz  "\000\000\000\000\000\000\000\000\b\000\000\000\000\000\000\000\b\000\000\000\000\000\000"
        .quad   __RNSNvYNCINvNtCs8BpVhDwHqJW_3std2rt10lang_startuE0INtNtNtCsjgY6bXVaRmE_4core3ops8function6FnOnceuE9call_once6vtableCsjWM3FVHqJkI_10experiment
        .quad   __RNCINvNtCs8BpVhDwHqJW_3std2rt10lang_startuE0CsjWM3FVHqJkI_10experiment
        .quad   __RNCINvNtCs8BpVhDwHqJW_3std2rt10lang_startuE0CsjWM3FVHqJkI_10experiment

        .section        __TEXT,__cstring,cstring_literals
l_anon.ffb3631f6cd35efadd1993fc3f34d432.1:
        .asciz  "/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/neon-mask-leaf-experiment/new.rs"

        .section        __DATA,__const
        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.2:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.1
        .asciz  "[\000\000\000\000\000\000\000b\000\000\000\n\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.3:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.1
        .asciz  "[\000\000\000\000\000\000\000[\000\000\000\031\000\000"

        .section        __TEXT,__cstring,cstring_literals
l_anon.ffb3631f6cd35efadd1993fc3f34d432.4:
        .asciz  "/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/neon-mask-leaf-experiment/old.rs"

        .section        __DATA,__const
        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.5:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.4
        .asciz  "[\000\000\000\000\000\000\000b\000\000\000\n\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.6:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.4
        .asciz  "[\000\000\000\000\000\000\000[\000\000\000\031\000\000"

        .section        __TEXT,__cstring,cstring_literals
l_anon.ffb3631f6cd35efadd1993fc3f34d432.7:
        .asciz  "\bCHECKED \300\001\n"

l_anon.ffb3631f6cd35efadd1993fc3f34d432.8:
        .asciz  "/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/neon-mask-leaf-experiment/experiment.rs"

        .section        __DATA,__const
        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.9:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.8
        .asciz  "b\000\000\000\000\000\000\000!\000\000\000\003\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.10:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.8
        .asciz  "b\000\000\000\000\000\000\000\024\000\000\000\006\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.11:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.8
        .asciz  "b\000\000\000\000\000\000\000\025\000\000\000\006\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.12:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.8
        .asciz  "b\000\000\000\000\000\000\000\027\000\000\000\006\000\000"

        .p2align        3, 0x0
l_anon.ffb3631f6cd35efadd1993fc3f34d432.13:
        .quad   l_anon.ffb3631f6cd35efadd1993fc3f34d432.8
        .asciz  "b\000\000\000\000\000\000\000\031\000\000\000\006\000\000"

.subsections_via_symbols
