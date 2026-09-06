/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/plan-scan-worker:    file format mach-o arm64

Disassembly of section __TEXT,__text:

0000000100562cc0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record>:
100562cc0:      stp x28, x27, [sp, #-0x60]!
100562cc4:      stp x26, x25, [sp, #0x10]
100562cc8:      stp x24, x23, [sp, #0x20]
100562ccc:      stp x22, x21, [sp, #0x30]
100562cd0:      stp x20, x19, [sp, #0x40]
100562cd4:      stp x29, x30, [sp, #0x50]
100562cd8:      add x29, sp, #0x50
100562cdc:      sub sp, sp, #0x5e0
100562ce0:      ldr xzr, [sp]
100562ce4:      str x1, [sp, #0x38]
100562ce8:      mov x10, x0
100562cec:      movi.2d v0, #0000000000000000
100562cf0:      str d0, [sp, #0x40]
100562cf4:      str wzr, [sp, #0x48]
100562cf8:      str d0, [sp, #0x68]
100562cfc:      str wzr, [sp, #0x70]
100562d00:      str d0, [sp, #0x90]
100562d04:      str wzr, [sp, #0x98]
100562d08:      str d0, [sp, #0xb8]
100562d0c:      str wzr, [sp, #0xc0]
100562d10:      str d0, [sp, #0xe0]
100562d14:      str wzr, [sp, #0xe8]
100562d18:      str d0, [sp, #0x108]
100562d1c:      str wzr, [sp, #0x110]
100562d20:      str d0, [sp, #0x130]
100562d24:      str wzr, [sp, #0x138]
100562d28:      str d0, [sp, #0x158]
100562d2c:      str wzr, [sp, #0x160]
100562d30:      str d0, [sp, #0x180]
100562d34:      str wzr, [sp, #0x188]
100562d38:      str d0, [sp, #0x1a8]
100562d3c:      str wzr, [sp, #0x1b0]
100562d40:      str d0, [sp, #0x1d0]
100562d44:      str wzr, [sp, #0x1d8]
100562d48:      str d0, [sp, #0x1f8]
100562d4c:      str wzr, [sp, #0x200]
100562d50:      str d0, [sp, #0x220]
100562d54:      str wzr, [sp, #0x228]
100562d58:      str d0, [sp, #0x248]
100562d5c:      str wzr, [sp, #0x250]
100562d60:      str d0, [sp, #0x270]
100562d64:      str wzr, [sp, #0x278]
100562d68:      str d0, [sp, #0x298]
100562d6c:      str wzr, [sp, #0x2a0]
100562d70:      str d0, [sp, #0x2c0]
100562d74:      str wzr, [sp, #0x2c8]
100562d78:      str d0, [sp, #0x2e8]
100562d7c:      str wzr, [sp, #0x2f0]
100562d80:      str d0, [sp, #0x310]
100562d84:      str wzr, [sp, #0x318]
100562d88:      str d0, [sp, #0x338]
100562d8c:      str wzr, [sp, #0x340]
100562d90:      str d0, [sp, #0x360]
100562d94:      str wzr, [sp, #0x368]
100562d98:      str d0, [sp, #0x388]
100562d9c:      str wzr, [sp, #0x390]
100562da0:      str d0, [sp, #0x3b0]
100562da4:      str wzr, [sp, #0x3b8]
100562da8:      str d0, [sp, #0x3d8]
100562dac:      str wzr, [sp, #0x3e0]
100562db0:      str d0, [sp, #0x400]
100562db4:      str wzr, [sp, #0x408]
100562db8:      str d0, [sp, #0x428]
100562dbc:      str wzr, [sp, #0x430]
100562dc0:      str d0, [sp, #0x450]
100562dc4:      str wzr, [sp, #0x458]
100562dc8:      str d0, [sp, #0x478]
100562dcc:      str wzr, [sp, #0x480]
100562dd0:      str d0, [sp, #0x4a0]
100562dd4:      str wzr, [sp, #0x4a8]
100562dd8:      str d0, [sp, #0x4c8]
100562ddc:      str wzr, [sp, #0x4d0]
100562de0:      str d0, [sp, #0x4f0]
100562de4:      str wzr, [sp, #0x4f8]
100562de8:      str d0, [sp, #0x518]
100562dec:      str wzr, [sp, #0x520]
100562df0:      ldr w19, [x0, #0x4]
100562df4:      adrp    x8, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100562df8:      add x8, x8, #0x94
100562dfc:      ldr w20, [x8]
100562e00:      adrp    x8, 0x101118000 <_perry_global_baseline_worker_ts__1>
100562e04:      add x8, x8, #0xec8
100562e08:      cmp w20, #0x300
100562e0c:      b.hs    0x100562ea4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
100562e10:      ldr x8, [x8]
100562e14:      cmn x8, #0x1
100562e18:      b.eq    0x100562e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
100562e1c:      mrs x9, TPIDRRO_EL0
100562e20:      and x9, x9, #0xfffffffffffffff8
100562e24:      ldr x0, [x9, x8, lsl #3]
100562e28:      cbz x0, 0x100562e8c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1cc>
100562e2c:      add x8, x0, x20, lsl #3
100562e30:      ldr x0, [x8, #0x1e8]
100562e34:      cbz x0, 0x100562ea4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x1e4>
100562e38:      ldr x0, [x0]
100562e3c:      cbz x0, 0x100562ec0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x200>
100562e40:      mov w8, #-0x40000001        ; =-1073741825
100562e44:      cmp w19, w8
100562e48:      b.gt    0x100562ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
100562e4c:      ldr x9, [x0, #0x5198]
100562e50:      ubfx    x8, x19, #15, #15
100562e54:      cmp x8, x9
100562e58:      b.hs    0x100562ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
100562e5c:      ldr x9, [x0, #0x5190]
100562e60:      ldr x8, [x9, x8, lsl #3]
100562e64:      cbz x8, 0x100562edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
100562e68:      ubfx    x9, x19, #5, #10
100562e6c:      ldr x8, [x8, x9, lsl #3]
100562e70:      cbz x8, 0x100562edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
100562e74:      and x9, x19, #0x1f
100562e78:      add x8, x8, x9, lsl #5
100562e7c:      ldrb    w9, [x8, #0x1c]
100562e80:      tbz w9, #0x0, 0x100562ed8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x218>
100562e84:      ldr x8, [x8]
100562e88:      b   0x100562edc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x21c>
100562e8c:      mov x21, x10
100562e90:      bl  0x100c8b6c8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
100562e94:      mov x10, x21
100562e98:      add x8, x0, x20, lsl #3
100562e9c:      ldr x0, [x8, #0x1e8]
100562ea0:      cbnz    x0, 0x100562e38 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x178>
100562ea4:      adrp    x0, 0x1010a3000 <_anon.7adc4553ee057240d1951d2053fb5027.1693+0x1c8>
100562ea8:      add x0, x0, #0x218
100562eac:      mov x20, x10
100562eb0:      bl  0x100c8ae04 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
100562eb4:      mov x10, x20
100562eb8:      ldr x0, [x0]
100562ebc:      cbnz    x0, 0x100562e40 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x180>
100562ec0:      mov x20, x10
100562ec4:      bl  0x100cb2b78 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
100562ec8:      mov x10, x20
100562ecc:      mov w8, #-0x40000001        ; =-1073741825
100562ed0:      cmp w19, w8
100562ed4:      b.le    0x100562e4c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x18c>
100562ed8:      mov x8, #0x0                ; =0
100562edc:      mov x23, #0x0               ; =0
100562ee0:      add x8, x8, #0x8
100562ee4:      str x8, [sp, #0x30]
100562ee8:      sub x28, x29, #0x80
100562eec:      add x8, x10, #0x10
100562ef0:      stp xzr, x8, [sp, #0x20]
100562ef4:      add x8, sp, #0x2c0
100562ef8:      add x8, x8, #0x8
100562efc:      stp x10, x8, [sp, #0x10]
100562f00:      mov w22, #0x2               ; =2
100562f04:      mov w27, #0x28              ; =40
100562f08:      mov w26, #0x2               ; =2
100562f0c:      ldr x8, [sp, #0x30]
100562f10:      ldr x1, [x8, x23, lsl #3]
100562f14:      sub x0, x29, #0x80
100562f18:      bl  0x10055d724 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12string_piece>
100562f1c:      ldur    w9, [x29, #-0x80]
100562f20:      cmn w9, #0x1
100562f24:      b.eq    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100562f28:      ldp w8, w19, [x29, #-0x7c]
100562f2c:      ldur    w25, [x29, #-0x74]
100562f30:      ldur    q0, [x28, #0x10]
100562f34:      stur    q0, [x29, #-0xf0]
100562f38:      ldur    x10, [x28, #0x20]
100562f3c:      stur    x10, [x29, #-0xe0]
100562f40:      add x11, sp, #0x40
100562f44:      madd    x11, x23, x27, x11
100562f48:      stp w9, w8, [x11]
100562f4c:      stp w19, w25, [x11, #0x8]
100562f50:      str q0, [x11, #0x10]
100562f54:      str x10, [x11, #0x20]
100562f58:      cmp w9, #0x2
100562f5c:      b.eq    0x100562f74 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2b4>
100562f60:      cmp w9, #0x1
100562f64:      b.eq    0x100562f7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
100562f68:      add w25, w19, #0x2
100562f6c:      add w19, w8, #0x2
100562f70:      b   0x100562f7c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x2bc>
100562f74:      mov x25, x8
100562f78:      mov x19, x8
100562f7c:      ldr x8, [sp, #0x28]
100562f80:      ldr x21, [x8, x23, lsl #3]
100562f84:      sub x0, x29, #0xd8
100562f88:      mov x1, x21
100562f8c:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
100562f90:      ldur    w8, [x29, #-0xd8]
100562f94:      cmn w8, #0x1
100562f98:      b.eq    0x100562ff4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x334>
100562f9c:      ldp w20, w21, [x29, #-0xd4]
100562fa0:      ldur    w9, [x29, #-0xcc]
100562fa4:      add x10, sp, #0x180
100562fa8:      madd    x10, x23, x27, x10
100562fac:      stp w8, w20, [x10]
100562fb0:      stp w21, w9, [x10, #0x8]
100562fb4:      sub x11, x29, #0xd8
100562fb8:      ldur    q0, [x11, #0x10]
100562fbc:      str q0, [x10, #0x10]
100562fc0:      ldur    x11, [x11, #0x20]
100562fc4:      str x11, [x10, #0x20]
100562fc8:      cmp w8, #0x2
100562fcc:      b.eq    0x1005631a4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x4e4>
100562fd0:      cmp w8, #0x1
100562fd4:      b.ne    0x1005631c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x500>
100562fd8:      mov x20, x9
100562fdc:      cmp x23, #0x0
100562fe0:      mov w8, #0x1                ; =1
100562fe4:      cinc    w8, w8, ne
100562fe8:      adds    w9, w19, w22
100562fec:      b.lo    0x100563204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
100562ff0:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100562ff4:      mov w8, #0x7ffd             ; =32765
100562ff8:      cmp x8, x21, lsr #48
100562ffc:      b.ne    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563000:      and x21, x21, #0xffffffffffff
100563004:      mov x0, x21
100563008:      bl  0x100580618 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
10056300c:      cbz x0, 0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100563010:      ldrb    w8, [x0]
100563014:      cmp w8, #0x1
100563018:      b.ne    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056301c:      ldrsb   w8, [x0, #0x1]
100563020:      tbnz    w8, #0x1f, 0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563024:      ldrh    w8, [x0, #0x2]
100563028:      tbnz    w8, #0xa, 0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056302c:      ldr w8, [x0, #0x4]
100563030:      cmp w8, #0x10
100563034:      b.lo    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563038:      ldr w20, [x21]
10056303c:      cmp w20, #0x10
100563040:      b.hi    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563044:      ldr w9, [x21, #0x4]
100563048:      cmp w20, w9
10056304c:      b.hi    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563050:      lsl x24, x20, #3
100563054:      add x9, x24, #0x10
100563058:      cmp x9, x8
10056305c:      b.hi    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563060:      mov x0, x21
100563064:      bl  0x10057def0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array6header35array_has_named_properties_resolved>
100563068:      tbnz    w0, #0x0, 0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056306c:      mov x0, x21
100563070:      bl  0x1003b1a24 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object15prototype_chain23object_static_prototype>
100563074:      cmp x0, #0x1
100563078:      b.eq    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056307c:      ldr x8, [sp, #0x20]
100563080:      add x10, x8, x20
100563084:      cmp x10, #0x10
100563088:      b.hi    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056308c:      add x8, sp, #0x180
100563090:      madd    x8, x23, x27, x8
100563094:      mov w9, #-0x1               ; =-1
100563098:      str w9, [x8]
10056309c:      ldr x9, [sp, #0x20]
1005630a0:      stp x9, x20, [x8, #0x8]
1005630a4:      cbz w20, 0x1005631e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x524>
1005630a8:      str x10, [sp]
1005630ac:      stp w26, w22, [sp, #0x8]
1005630b0:      mov x22, #0x0               ; =0
1005630b4:      mov x10, x9
1005630b8:      mov w9, #0x28               ; =40
1005630bc:      add x27, x21, #0x8
1005630c0:      ldr x8, [sp, #0x18]
1005630c4:      madd    x26, x10, x9, x8
1005630c8:      mov w20, #0x2               ; =2
1005630cc:      mov w21, #0x2               ; =2
1005630d0:      ldr x1, [x27, x22]
1005630d4:      sub x0, x29, #0x80
1005630d8:      bl  0x10055d460 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat12scalar_piece>
1005630dc:      ldur    w8, [x29, #-0x80]
1005630e0:      cmn w8, #0x1
1005630e4:      b.eq    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1005630e8:      ldp w10, w9, [x29, #-0x7c]
1005630ec:      ldur    w11, [x29, #-0x74]
1005630f0:      ldur    q0, [x28, #0x10]
1005630f4:      stur    q0, [x29, #-0xb0]
1005630f8:      ldur    x12, [x28, #0x20]
1005630fc:      stur    x12, [x29, #-0xa0]
100563100:      stp w8, w10, [x26, #-0x8]
100563104:      stp w9, w11, [x26]
100563108:      stur    q0, [x26, #0x8]
10056310c:      str x12, [x26, #0x18]
100563110:      cbz w8, 0x100563134 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x474>
100563114:      cmp w8, #0x1
100563118:      csel    w8, w11, w10, eq
10056311c:      csel    w10, w9, w10, eq
100563120:      cmp x22, #0x0
100563124:      cset    w9, ne
100563128:      adds    w10, w10, w21
10056312c:      b.lo    0x10056314c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x48c>
100563130:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563134:      add w10, w10, #0x2
100563138:      add w8, w9, #0x2
10056313c:      cmp x22, #0x0
100563140:      cset    w9, ne
100563144:      adds    w10, w10, w21
100563148:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056314c:      adds    w21, w10, w9
100563150:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563154:      mov x0, #0x0                ; =0
100563158:      adds    w8, w8, w20
10056315c:      cset    w10, hs
100563160:      adds    w20, w8, w9
100563164:      cset    w8, hs
100563168:      tbnz    w10, #0x0, 0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
10056316c:      tbnz    w8, #0x0, 0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100563170:      add x22, x22, #0x8
100563174:      add x26, x26, #0x28
100563178:      cmp x24, x22
10056317c:      b.ne    0x1005630d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x410>
100563180:      ldr x8, [sp]
100563184:      str x8, [sp, #0x20]
100563188:      ldp w26, w22, [sp, #0x8]
10056318c:      cmp x23, #0x0
100563190:      mov w8, #0x1                ; =1
100563194:      cinc    w8, w8, ne
100563198:      adds    w9, w19, w22
10056319c:      b.lo    0x100563204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1005631a0:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1005631a4:      mov x21, x20
1005631a8:      cmp x23, #0x0
1005631ac:      mov w8, #0x1                ; =1
1005631b0:      cinc    w8, w8, ne
1005631b4:      adds    w9, w19, w22
1005631b8:      b.lo    0x100563204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1005631bc:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1005631c0:      add w8, w20, #0x2
1005631c4:      add w20, w21, #0x2
1005631c8:      mov x21, x8
1005631cc:      cmp x23, #0x0
1005631d0:      mov w8, #0x1                ; =1
1005631d4:      cinc    w8, w8, ne
1005631d8:      adds    w9, w19, w22
1005631dc:      b.lo    0x100563204 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x544>
1005631e0:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
1005631e4:      mov w20, #0x2               ; =2
1005631e8:      mov w21, #0x2               ; =2
1005631ec:      str x10, [sp, #0x20]
1005631f0:      cmp x23, #0x0
1005631f4:      mov w8, #0x1                ; =1
1005631f8:      cinc    w8, w8, ne
1005631fc:      adds    w9, w19, w22
100563200:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563204:      adds    w9, w21, w9
100563208:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056320c:      adds    w11, w9, w8
100563210:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563214:      adds    w9, w25, w26
100563218:      b.hs    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056321c:      mov x0, #0x0                ; =0
100563220:      adds    w9, w20, w9
100563224:      cset    w10, hs
100563228:      adds    w24, w9, w8
10056322c:      cset    w8, hs
100563230:      tbnz    w10, #0x0, 0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100563234:      tbnz    w8, #0x0, 0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100563238:      mov x20, x11
10056323c:      add x23, x23, #0x1
100563240:      ldr x8, [sp, #0x38]
100563244:      cmp x23, x8
100563248:      mov x22, x11
10056324c:      mov x26, x24
100563250:      mov w27, #0x28              ; =40
100563254:      b.ne    0x100562f0c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x24c>
100563258:      adrp    x19, 0x101118000 <_perry_global_baseline_worker_ts__1>
10056325c:      add x19, x19, #0xec8
100563260:      ldr x8, [x19]
100563264:      cmn x8, #0x1
100563268:      b.eq    0x10056329c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
10056326c:      mrs x9, TPIDRRO_EL0
100563270:      and x9, x9, #0xfffffffffffffff8
100563274:      ldr x8, [x9, x8, lsl #3]
100563278:      cbz x8, 0x10056329c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x5dc>
10056327c:      ldr x8, [x8, #0x19e8]
100563280:      cbz x8, 0x1005632d0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x610>
100563284:      ldr x9, [x8]
100563288:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
10056328c:      cmp x9, x10
100563290:      b.hs    0x100563870 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbb0>
100563294:      ldr x21, [x8, #0x18]
100563298:      b   0x1005632e0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x620>
10056329c:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
1005632a0:      add x0, x0, #0x550
1005632a4:      ldr x8, [x0]
1005632a8:      blr x8
1005632ac:      ldrb    w8, [x0, #0x20]
1005632b0:      cbnz    w8, 0x100563818 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb58>
1005632b4:      ldr x8, [x0]
1005632b8:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1005632bc:      cmp x8, x9
1005632c0:      ldr x20, [sp, #0x10]
1005632c4:      b.hs    0x100563854 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb94>
1005632c8:      ldr x21, [x0, #0x18]
1005632cc:      b   0x1005632e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x624>
1005632d0:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
1005632d4:      add x0, x0, #0x838
1005632d8:      bl  0x1001353ac <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvMs_NtB24_15runtime_handlesNtB3i_18RuntimeHandleScope3new0jEB28_>
1005632dc:      mov x21, x0
1005632e0:      ldr x20, [sp, #0x10]
1005632e4:      stur    x21, [x29, #-0x90]
1005632e8:      mov x8, #0x7ffd000000000000 ; =9222527611924643840
1005632ec:      stp x20, x8, [x29, #-0x78]
1005632f0:      mov w8, #0x1                ; =1
1005632f4:      stur    x8, [x29, #-0x80]
1005632f8:      sub x0, x29, #0x80
1005632fc:      bl  0x1005286e0 <__RNvMs_NtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handlesNtB4_18RuntimeHandleScope4push>
100563300:      mov x24, x0
100563304:      stur    x0, [x29, #-0x88]
100563308:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10056330c:      add x0, x0, #0x4c0
100563310:      ldr x8, [x0]
100563314:      blr x8
100563318:      strb    wzr, [x0]
10056331c:      mov x0, x20
100563320:      bl  0x1002f08c0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json22stringify_tojson_probe25to_json_definitely_absent>
100563324:      tbz w0, #0x0, 0x1005633c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x708>
100563328:      mov x0, x22
10056332c:      bl  0x1005356ec <__RNvNtCs5gMwpk3Cs4e_13perry_runtime6string20string_storage_alloc>
100563330:      mov x23, x1
100563334:      stp w26, w22, [x0]
100563338:      stp wzr, wzr, [x0, #0xc]
10056333c:      str w22, [x0, #0x8]
100563340:      ldr x8, [x19]
100563344:      cmn x8, #0x1
100563348:      str x0, [sp, #0x20]
10056334c:      b.eq    0x10056340c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
100563350:      mrs x9, TPIDRRO_EL0
100563354:      and x9, x9, #0xfffffffffffffff8
100563358:      ldr x8, [x9, x8, lsl #3]
10056335c:      cbz x8, 0x10056340c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x74c>
100563360:      ldr x8, [x8, #0x19e8]
100563364:      cbz x8, 0x10056353c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x87c>
100563368:      ldr x9, [x8]
10056336c:      mov x10, #0x7fffffffffffffff ; =9223372036854775807
100563370:      cmp x9, x10
100563374:      b.hs    0x100563904 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc44>
100563378:      add x10, x9, #0x1
10056337c:      str x10, [x8]
100563380:      ldr x10, [x8, #0x18]
100563384:      cmp x24, x10
100563388:      b.hs    0x100563814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
10056338c:      ldr x10, [x8, #0x10]
100563390:      mov w11, #0x18              ; =24
100563394:      madd    x10, x24, x11, x10
100563398:      ldr x11, [x10]
10056339c:      cmp x11, #0x1
1005633a0:      b.ne    0x100563910 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc50>
1005633a4:      ldr x22, [x10, #0x8]
1005633a8:      str x9, [x8]
1005633ac:      ldr w19, [x22, #0x4]
1005633b0:      adrp    x8, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
1005633b4:      add x8, x8, #0x94
1005633b8:      ldr w20, [x8]
1005633bc:      cmp w20, #0x300
1005633c0:      b.lo    0x100563480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
1005633c4:      b   0x1005635b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1005633c8:      ldr x8, [x19]
1005633cc:      cmn x8, #0x1
1005633d0:      b.eq    0x100563508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
1005633d4:      mrs x9, TPIDRRO_EL0
1005633d8:      and x9, x9, #0xfffffffffffffff8
1005633dc:      ldr x8, [x9, x8, lsl #3]
1005633e0:      cbz x8, 0x100563508 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x848>
1005633e4:      ldr x8, [x8, #0x19e8]
1005633e8:      cbz x8, 0x10056356c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8ac>
1005633ec:      ldr x9, [x8]
1005633f0:      cbnz    x9, 0x10056387c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
1005633f4:      ldr x9, [x8, #0x18]
1005633f8:      cmp x21, x9
1005633fc:      b.hi    0x100563404 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x744>
100563400:      str x21, [x8, #0x18]
100563404:      str xzr, [x8]
100563408:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056340c:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
100563410:      add x0, x0, #0x550
100563414:      ldr x8, [x0]
100563418:      blr x8
10056341c:      ldrb    w8, [x0, #0x20]
100563420:      cbnz    w8, 0x100563888 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbc8>
100563424:      ldr x8, [x0]
100563428:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
10056342c:      cmp x8, x9
100563430:      b.hs    0x1005638b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbf8>
100563434:      add x9, x8, #0x1
100563438:      str x9, [x0]
10056343c:      ldr x9, [x0, #0x18]
100563440:      cmp x24, x9
100563444:      b.hs    0x100563814 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb54>
100563448:      ldr x9, [x0, #0x10]
10056344c:      mov w10, #0x18              ; =24
100563450:      madd    x9, x24, x10, x9
100563454:      ldr x10, [x9]
100563458:      cmp x10, #0x1
10056345c:      b.ne    0x100563860 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xba0>
100563460:      ldr x22, [x9, #0x8]
100563464:      str x8, [x0]
100563468:      ldr w19, [x22, #0x4]
10056346c:      adrp    x8, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100563470:      add x8, x8, #0x94
100563474:      ldr w20, [x8]
100563478:      cmp w20, #0x300
10056347c:      b.hs    0x1005635b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
100563480:      adrp    x8, 0x101118000 <_perry_global_baseline_worker_ts__1>
100563484:      add x8, x8, #0xec8
100563488:      ldr x8, [x8]
10056348c:      cmn x8, #0x1
100563490:      b.eq    0x1005635a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
100563494:      mrs x9, TPIDRRO_EL0
100563498:      and x9, x9, #0xfffffffffffffff8
10056349c:      ldr x0, [x9, x8, lsl #3]
1005634a0:      cbz x0, 0x1005635a0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8e0>
1005634a4:      add x8, x0, x20, lsl #3
1005634a8:      ldr x0, [x8, #0x1e8]
1005634ac:      cbz x0, 0x1005635b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
1005634b0:      ldr x0, [x0]
1005634b4:      cbz x0, 0x1005635c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x904>
1005634b8:      mov w8, #-0x40000001        ; =-1073741825
1005634bc:      cmp w19, w8
1005634c0:      str x21, [sp, #0x18]
1005634c4:      b.gt    0x1005635d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
1005634c8:      ldr x9, [x0, #0x5198]
1005634cc:      ubfx    x8, x19, #15, #15
1005634d0:      cmp x8, x9
1005634d4:      b.hs    0x1005635d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
1005634d8:      ldr x9, [x0, #0x5190]
1005634dc:      ldr x8, [x9, x8, lsl #3]
1005634e0:      cbz x8, 0x1005635dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1005634e4:      ubfx    x9, x19, #5, #10
1005634e8:      ldr x8, [x8, x9, lsl #3]
1005634ec:      cbz x8, 0x1005635dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
1005634f0:      and x9, x19, #0x1f
1005634f4:      add x8, x8, x9, lsl #5
1005634f8:      ldrb    w9, [x8, #0x1c]
1005634fc:      tbz w9, #0x0, 0x1005635d8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x918>
100563500:      ldr x8, [x8]
100563504:      b   0x1005635dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x91c>
100563508:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
10056350c:      add x0, x0, #0x550
100563510:      ldr x8, [x0]
100563514:      blr x8
100563518:      ldrb    w8, [x0, #0x20]
10056351c:      cbnz    w8, 0x1005638c4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc04>
100563520:      ldr x8, [x0]
100563524:      cbnz    x8, 0x100563940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
100563528:      ldr x8, [x0, #0x18]
10056352c:      cmp x21, x8
100563530:      b.hi    0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
100563534:      str x21, [x0, #0x18]
100563538:      b   0x10056357c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8bc>
10056353c:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
100563540:      add x0, x0, #0x838
100563544:      sub x1, x29, #0x88
100563548:      bl  0x1001351d0 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCINvMs2_NtB24_15runtime_handlesNtB3k_13RuntimeHandle9with_slotPhNCINvB3g_17get_raw_const_ptrhE0E0B4c_EB28_>
10056354c:      mov x22, x0
100563550:      ldr w19, [x0, #0x4]
100563554:      adrp    x8, 0x101119000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime3tls19TLS_CLIENT_METADATA+0x38>
100563558:      add x8, x8, #0x94
10056355c:      ldr w20, [x8]
100563560:      cmp w20, #0x300
100563564:      b.lo    0x100563480 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7c0>
100563568:      b   0x1005635b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8f0>
10056356c:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
100563570:      add x0, x0, #0x838
100563574:      sub x1, x29, #0x90
100563578:      bl  0x100135788 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
10056357c:      mov x0, #0x0                ; =0
100563580:      add sp, sp, #0x5e0
100563584:      ldp x29, x30, [sp, #0x50]
100563588:      ldp x20, x19, [sp, #0x40]
10056358c:      ldp x22, x21, [sp, #0x30]
100563590:      ldp x24, x23, [sp, #0x20]
100563594:      ldp x26, x25, [sp, #0x10]
100563598:      ldp x28, x27, [sp], #0x60
10056359c:      ret
1005635a0:      bl  0x100c8b6c8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1005635a4:      add x8, x0, x20, lsl #3
1005635a8:      ldr x0, [x8, #0x1e8]
1005635ac:      cbnz    x0, 0x1005634b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f0>
1005635b0:      adrp    x0, 0x1010a3000 <_anon.7adc4553ee057240d1951d2053fb5027.1693+0x1c8>
1005635b4:      add x0, x0, #0x218
1005635b8:      bl  0x100c8ae04 <__RNvMs5_NtCs5gMwpk3Cs4e_13perry_runtime7tls_hotINtB5_6HotKeyNtNtNtB7_7closure8registry14DispatchRecentE8get_slowB7_>
1005635bc:      ldr x0, [x0]
1005635c0:      cbnz    x0, 0x1005634b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x7f8>
1005635c4:      bl  0x100cb2b78 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5state10init_state>
1005635c8:      mov w8, #-0x40000001        ; =-1073741825
1005635cc:      cmp w19, w8
1005635d0:      str x21, [sp, #0x18]
1005635d4:      b.le    0x1005634c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x808>
1005635d8:      mov x8, #0x0                ; =0
1005635dc:      mov x19, #0x0               ; =0
1005635e0:      mov w9, #0x7b               ; =123
1005635e4:      strb    w9, [x23]
1005635e8:      add x26, x8, #0x8
1005635ec:      add x25, x22, #0x10
1005635f0:      add x8, sp, #0x2c0
1005635f4:      add x8, x8, #0x28
1005635f8:      stp x8, x26, [sp, #0x28]
1005635fc:      mov w21, #0x1               ; =1
100563600:      add x27, sp, #0x40
100563604:      mov w28, #0x3a              ; =58
100563608:      mov w20, #0x2c              ; =44
10056360c:      b   0x100563634 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
100563610:      mov w8, #0x5d               ; =93
100563614:      strb    w8, [x23, x26]
100563618:      add x21, x26, #0x1
10056361c:      ldp x26, x8, [sp, #0x30]
100563620:      add x27, sp, #0x40
100563624:      mov w28, #0x3a              ; =58
100563628:      add x19, x19, #0x1
10056362c:      cmp x19, x8
100563630:      b.eq    0x100563760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
100563634:      cbz x19, 0x100563640 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x980>
100563638:      strb    w20, [x23, x21]
10056363c:      add x21, x21, #0x1
100563640:      add x8, x19, x19, lsl #2
100563644:      lsl x24, x8, #3
100563648:      ldr x1, [x26, x19, lsl #3]
10056364c:      add x0, x27, x24
100563650:      add x2, x23, x21
100563654:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100563658:      add x8, x0, x21
10056365c:      strb    w28, [x23, x8]
100563660:      add x22, x8, #0x1
100563664:      ldr x1, [x25, x19, lsl #3]
100563668:      add x9, sp, #0x180
10056366c:      add x0, x9, x24
100563670:      ldr w9, [x0]
100563674:      cmn w9, #0x1
100563678:      b.eq    0x10056369c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x9dc>
10056367c:      add x2, x23, x22
100563680:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100563684:      add x21, x0, x22
100563688:      add x19, x19, #0x1
10056368c:      ldr x8, [sp, #0x38]
100563690:      cmp x19, x8
100563694:      b.ne    0x100563634 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x974>
100563698:      b   0x100563760 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaa0>
10056369c:      ldp x24, x21, [x0, #0x8]
1005636a0:      add x26, x8, #0x2
1005636a4:      mov w8, #0x5b               ; =91
1005636a8:      strb    w8, [x23, x22]
1005636ac:      cbz x21, 0x100563610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1005636b0:      cmp x24, #0xf
1005636b4:      b.hi    0x10056394c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc8c>
1005636b8:      and x22, x1, #0xffffffffffff
1005636bc:      add x8, sp, #0x2c0
1005636c0:      mov w9, #0x28               ; =40
1005636c4:      madd    x8, x24, x9, x8
1005636c8:      ldp q0, q1, [x8]
1005636cc:      stp q0, q1, [x29, #-0x80]
1005636d0:      ldr x8, [x8, #0x20]
1005636d4:      stur    x8, [x29, #-0x60]
1005636d8:      ldr x1, [x22, #0x8]
1005636dc:      sub x0, x29, #0x80
1005636e0:      add x2, x23, x26
1005636e4:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
1005636e8:      add x26, x0, x26
1005636ec:      subs    x28, x21, #0x1
1005636f0:      b.eq    0x100563610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
1005636f4:      add x21, x22, #0x10
1005636f8:      cmp x24, #0x10
1005636fc:      mov w8, #0x10               ; =16
100563700:      csel    x8, x24, x8, lo
100563704:      mov w9, #0x28               ; =40
100563708:      sub x27, x8, #0xf
10056370c:      add x22, x24, #0x1
100563710:      ldr x8, [sp, #0x28]
100563714:      madd    x24, x24, x9, x8
100563718:      strb    w20, [x23, x26]
10056371c:      cbz x27, 0x100563950 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc90>
100563720:      add x26, x26, #0x1
100563724:      ldp q0, q1, [x24]
100563728:      stp q0, q1, [x29, #-0x80]
10056372c:      ldr x8, [x24, #0x20]
100563730:      stur    x8, [x29, #-0x60]
100563734:      ldr x1, [x21], #0x8
100563738:      sub x0, x29, #0x80
10056373c:      add x2, x23, x26
100563740:      bl  0x10055c638 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json14stringify_flat10emit_piece>
100563744:      add x26, x0, x26
100563748:      add x27, x27, #0x1
10056374c:      add x22, x22, #0x1
100563750:      add x24, x24, #0x28
100563754:      subs    x28, x28, #0x1
100563758:      b.ne    0x100563718 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xa58>
10056375c:      b   0x100563610 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x950>
100563760:      mov w8, #0x7d               ; =125
100563764:      strb    w8, [x23, x21]
100563768:      adrp    x8, 0x101118000 <_perry_global_baseline_worker_ts__1>
10056376c:      add x8, x8, #0xec8
100563770:      ldr x8, [x8]
100563774:      cmn x8, #0x1
100563778:      b.eq    0x1005637b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
10056377c:      mrs x9, TPIDRRO_EL0
100563780:      and x9, x9, #0xfffffffffffffff8
100563784:      ldr x8, [x9, x8, lsl #3]
100563788:      cbz x8, 0x1005637b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf8>
10056378c:      ldr x8, [x8, #0x19e8]
100563790:      ldr x10, [sp, #0x18]
100563794:      cbz x8, 0x1005637f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb30>
100563798:      ldr x9, [x8]
10056379c:      cbnz    x9, 0x10056387c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xbbc>
1005637a0:      ldr x9, [x8, #0x18]
1005637a4:      cmp x10, x9
1005637a8:      b.hi    0x1005637b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xaf0>
1005637ac:      str x10, [x8, #0x18]
1005637b0:      str xzr, [x8]
1005637b4:      b   0x100563800 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1005637b8:      adrp    x0, 0x10111f000 <__RNvNCNKNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6census3SEQ0s_023___RUST_STD_INTERNAL_VAL+0x8>
1005637bc:      add x0, x0, #0x550
1005637c0:      ldr x8, [x0]
1005637c4:      blr x8
1005637c8:      ldrb    w8, [x0, #0x20]
1005637cc:      ldr x20, [sp, #0x18]
1005637d0:      cbnz    w8, 0x1005638f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc30>
1005637d4:      ldr x8, [x0]
1005637d8:      cbnz    x8, 0x100563940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
1005637dc:      ldr x8, [x0, #0x18]
1005637e0:      cmp x20, x8
1005637e4:      b.hi    0x100563800 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1005637e8:      str x20, [x0, #0x18]
1005637ec:      b   0x100563800 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb40>
1005637f0:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
1005637f4:      add x0, x0, #0x838
1005637f8:      sub x1, x29, #0x90
1005637fc:      bl  0x100135788 <__RINvMs2_NtNtCs8BpVhDwHqJW_3std6thread5localINtB6_8LocalKeyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtCsctvjasLqLe9_5alloc3vec3VecNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots17RuntimeHandleSlotEEE4withNCNvXs1_NtB24_15runtime_handlesNtB3j_18RuntimeHandleScopeNtNtNtBZ_3ops4drop4Drop4drop0uEB28_>
100563800:      mov x1, #0x7fff000000000000 ; =9223090561878065152
100563804:      ldr x8, [sp, #0x20]
100563808:      bfxil   x1, x8, #0, #48
10056380c:      mov w0, #0x1                ; =1
100563810:      b   0x100563580 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x8c0>
100563814:      bl  0x100ca9614 <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles23handle_used_after_scope>
100563818:      cmp w8, #0x1
10056381c:      b.ne    0x1005638f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
100563820:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
100563824:      add x1, x1, #0x9c4
100563828:      mov x21, x0
10056382c:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100563830:      mov x0, x21
100563834:      strb    wzr, [x21, #0x20]
100563838:      mov x22, x20
10056383c:      mov x26, x24
100563840:      ldr x8, [x21]
100563844:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
100563848:      cmp x8, x9
10056384c:      ldr x20, [sp, #0x10]
100563850:      b.lo    0x1005632c8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x608>
100563854:      adrp    x0, 0x101088000 <_anon.68a532d94142320e15103d7866c451bd.21>
100563858:      add x0, x0, #0x468
10056385c:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100563860:      adrp    x0, 0x100dab000 <_anon.80eb82dabe382127be861d2f5954db24.3+0x22a0>
100563864:      add x0, x0, #0xbf0
100563868:      mov w1, #0xb                ; =11
10056386c:      bl  0x100ca95dc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100563870:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
100563874:      add x0, x0, #0x918
100563878:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
10056387c:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
100563880:      add x0, x0, #0x9f0
100563884:      bl  0x100c7f2ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
100563888:      cmp w8, #0x2
10056388c:      b.eq    0x1005638f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
100563890:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
100563894:      add x1, x1, #0x9c4
100563898:      mov x22, x0
10056389c:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1005638a0:      mov x0, x22
1005638a4:      strb    wzr, [x22, #0x20]
1005638a8:      ldr x8, [x22]
1005638ac:      mov x9, #0x7fffffffffffffff ; =9223372036854775807
1005638b0:      cmp x8, x9
1005638b4:      b.lo    0x100563434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x774>
1005638b8:      adrp    x0, 0x101087000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1005638bc:      add x0, x0, #0xf70
1005638c0:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
1005638c4:      cmp w8, #0x2
1005638c8:      b.eq    0x1005638f8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc38>
1005638cc:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
1005638d0:      add x1, x1, #0x9c4
1005638d4:      mov x19, x0
1005638d8:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
1005638dc:      mov x0, x19
1005638e0:      strb    wzr, [x19, #0x20]
1005638e4:      ldr x8, [x19]
1005638e8:      cbz x8, 0x100563528 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0x868>
1005638ec:      b   0x100563940 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc80>
1005638f0:      cmp w8, #0x2
1005638f4:      b.ne    0x100563920 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xc60>
1005638f8:      adrp    x0, 0x101087000 <_anon.80eb82dabe382127be861d2f5954db24.4+0x1348>
1005638fc:      add x0, x0, #0xed8
100563900:      bl  0x100cc53dc <__RNvNtNtCs8BpVhDwHqJW_3std6thread5local18panic_access_error>
100563904:      adrp    x0, 0x10109f000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value9to_string25SKIP_TO_PRIMITIVE_ONESHOT>
100563908:      add x0, x0, #0x8a0
10056390c:      bl  0x100c7f2dc <__RNvNtCsjgY6bXVaRmE_4core4cell30panic_already_mutably_borrowed>
100563910:      adrp    x0, 0x100dcd000 <_anon.e80c450fb99efd852d6d235001180335.1329+0x8>
100563914:      add x0, x0, #0xeac
100563918:      mov w1, #0xb                ; =11
10056391c:      bl  0x100ca95dc <__RNvNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc5roots15runtime_handles20handle_kind_mismatch>
100563920:      adrp    x1, 0x1005e1000 <__RINvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local6native5eager7destroyINtNtCsjgY6bXVaRmE_4core4cell7RefCellINtNtB1a_6option6OptionNtNtNtCs5gMwpk3Cs4e_13perry_runtime2gc6policy15BudgetedGcCycleEEEB29_+0x3a4>
100563924:      add x1, x1, #0x9c4
100563928:      mov x19, x0
10056392c:      bl  0x100b8d59c <__RNvNtNtNtNtCs8BpVhDwHqJW_3std3sys12thread_local11destructors4list8register>
100563930:      mov x0, x19
100563934:      strb    wzr, [x19, #0x20]
100563938:      ldr x8, [x19]
10056393c:      cbz x8, 0x1005637dc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime4json23stringify_record_output11emit_record+0xb1c>
100563940:      adrp    x0, 0x10108d000 <_anon.68a532d94142320e15103d7866c451bd.1142>
100563944:      add x0, x0, #0x270
100563948:      bl  0x100c7f2ac <__RNvNtCsjgY6bXVaRmE_4core4cell22panic_already_borrowed>
10056394c:      mov x22, x24
100563950:      adrp    x2, 0x1010a0000 <_anon.9bd75d14e3ca4089e03b47eaf962fb16.802+0x18>
100563954:      add x2, x2, #0xd0
100563958:      mov x0, x22
10056395c:      mov w1, #0x10               ; =16
100563960:      bl  0x100c7f40c <__RNvNtCsjgY6bXVaRmE_4core9panicking18panic_bounds_check>
