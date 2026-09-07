/Users/amlug/projects/perry/codex-json-fastpaths-artifacts/shape-plans-worker:  file format mach-o arm64

Disassembly of section __TEXT,__text:

00000001002e3fb8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array>:
1002e3fb8:      stp x26, x25, [sp, #-0x50]!
1002e3fbc:      stp x24, x23, [sp, #0x10]
1002e3fc0:      stp x22, x21, [sp, #0x20]
1002e3fc4:      stp x20, x19, [sp, #0x30]
1002e3fc8:      stp x29, x30, [sp, #0x40]
1002e3fcc:      add x29, sp, #0x40
1002e3fd0:      mov x19, x0
1002e3fd4:      ldr x23, [x19, #0x20]!
1002e3fd8:      cbz x23, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e3fdc:      lsr x8, x23, #51
1002e3fe0:      mov x21, x23
1002e3fe4:      cmp x8, #0xfff
1002e3fe8:      b.lo    0x1002e4000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x48>
1002e3fec:      mov w8, #0x7ffc             ; =32764
1002e3ff0:      cmp x8, x23, lsr #48
1002e3ff4:      b.eq    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e3ff8:      ands    x21, x23, #0xffffffffffff
1002e3ffc:      b.eq    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e4000:      and x8, x21, #0xfffffffffff00000
1002e4004:      lsr x9, x21, #47
1002e4008:      cmp x9, #0x0
1002e400c:      ccmp    x8, #0x0, #0x4, eq
1002e4010:      b.eq    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e4014:      tst x21, #0x3
1002e4018:      ccmp    x21, #0x7, #0x0, eq
1002e401c:      mov x20, x0
1002e4020:      b.ls    0x1002e4130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1002e4024:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e4028:      add x8, x8, #0x4e8
1002e402c:      ldr x8, [x8]
1002e4030:      cmn x8, #0x1
1002e4034:      b.eq    0x1002e4434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
1002e4038:      mrs x9, TPIDRRO_EL0
1002e403c:      and x9, x9, #0xfffffffffffffff8
1002e4040:      ldr x8, [x9, x8, lsl #3]
1002e4044:      cbz x8, 0x1002e4434 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x47c>
1002e4048:      lsr x1, x21, #20
1002e404c:      ldr x8, [x8, #0x10]
1002e4050:      ldrb    w9, [x8, #0x28]
1002e4054:      tbz w9, #0x0, 0x1002e4074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1002e4058:      ldr x9, [x8, #0x20]
1002e405c:      cmp x9, x1
1002e4060:      b.ne    0x1002e4074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1002e4064:      ldp x9, x10, [x8]
1002e4068:      cmp x9, x21
1002e406c:      ccmp    x10, x21, #0x0, ls
1002e4070:      b.hi    0x1002e40f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1002e4074:      ldrb    w9, [x8, #0x58]
1002e4078:      cbz w9, 0x1002e4098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
1002e407c:      ldr x9, [x8, #0x50]
1002e4080:      cmp x9, x1
1002e4084:      b.ne    0x1002e4098 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xe0>
1002e4088:      ldp x9, x10, [x8, #0x30]
1002e408c:      cmp x9, x21
1002e4090:      ccmp    x10, x21, #0x0, ls
1002e4094:      b.hi    0x1002e40e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x12c>
1002e4098:      ldrb    w9, [x8, #0x88]
1002e409c:      cbz w9, 0x1002e40bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
1002e40a0:      ldr x9, [x8, #0x80]
1002e40a4:      cmp x9, x1
1002e40a8:      b.ne    0x1002e40bc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x104>
1002e40ac:      ldp x9, x10, [x8, #0x60]
1002e40b0:      cmp x9, x21
1002e40b4:      ccmp    x10, x21, #0x0, ls
1002e40b8:      b.hi    0x1002e40ec <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x134>
1002e40bc:      ldrb    w9, [x8, #0xb8]
1002e40c0:      cbz w9, 0x1002e40fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1002e40c4:      ldr x9, [x8, #0xb0]
1002e40c8:      cmp x9, x1
1002e40cc:      b.ne    0x1002e40fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1002e40d0:      ldp x9, x10, [x8, #0x90]!
1002e40d4:      cmp x9, x21
1002e40d8:      ccmp    x10, x21, #0x0, ls
1002e40dc:      b.hi    0x1002e40f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1002e40e0:      b   0x1002e40fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x144>
1002e40e4:      add x8, x8, #0x30
1002e40e8:      b   0x1002e40f0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x138>
1002e40ec:      add x8, x8, #0x60
1002e40f0:      ldrb    w8, [x8, #0x19]
1002e40f4:      cmp w8, #0xff
1002e40f8:      b.ne    0x1002e4110 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x158>
1002e40fc:      mov x0, x21
1002e4100:      bl  0x100889a20 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5arena9page_meta33classify_heap_generation_uncached>
1002e4104:      mov x8, x0
1002e4108:      mov x0, x20
1002e410c:      and w8, w8, #0xff
1002e4110:      cbz w8, 0x1002e4130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1002e4114:      ldurb   w8, [x21, #-0x8]
1002e4118:      ldurb   w9, [x21, #-0x7]
1002e411c:      mov w10, #0x82              ; =130
1002e4120:      and w9, w9, w10
1002e4124:      cmp w9, #0x2
1002e4128:      ccmp    w8, #0x1, #0x0, eq
1002e412c:      b.eq    0x1002e4250 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x298>
1002e4130:      mov x0, x21
1002e4134:      bl  0x1002d386c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1002e4138:      cbz x0, 0x1002e4168 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x1b0>
1002e413c:      ldrb    w9, [x0]
1002e4140:      cmp w9, #0x1
1002e4144:      b.ne    0x1002e420c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x254>
1002e4148:      ldrsb   w8, [x0, #0x1]
1002e414c:      tbnz    w8, #0x1f, 0x1002e427c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2c4>
1002e4150:      mov x8, x0
1002e4154:      mov x0, x20
1002e4158:      ldp w10, w9, [x21]
1002e415c:      cmp w10, w9
1002e4160:      b.hi    0x1002e4314 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x35c>
1002e4164:      b   0x1002e4330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
1002e4168:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
1002e416c:      add x8, x8, #0x814
1002e4170:      ldaprb  w8, [x8]
1002e4174:      cbz w8, 0x1002e41b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1002e4178:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e417c:      add x8, x8, #0x238
1002e4180:      ldapr   x9, [x8]
1002e4184:      cmp x9, x21
1002e4188:      b.hi    0x1002e41b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1002e418c:      ldapur  x8, [x8, #0x8]
1002e4190:      cmp x8, x21
1002e4194:      b.lo    0x1002e41b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1002e4198:      mov x24, x0
1002e419c:      mov x0, x21
1002e41a0:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1002e41a4:      mov x8, x0
1002e41a8:      mov x0, x24
1002e41ac:      tbz w8, #0x0, 0x1002e41b8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x200>
1002e41b0:      mov x8, #0x0                ; =0
1002e41b4:      b   0x1002e4300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
1002e41b8:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
1002e41bc:      add x8, x8, #0xb78
1002e41c0:      ldaprb  w8, [x8]
1002e41c4:      cbz w8, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e41c8:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
1002e41cc:      add x8, x8, #0xd88
1002e41d0:      ldapr   x9, [x8]
1002e41d4:      cmp x9, x21
1002e41d8:      b.hi    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e41dc:      ldapur  x8, [x8, #0x8]
1002e41e0:      cmp x8, x21
1002e41e4:      b.lo    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e41e8:      mov x24, x0
1002e41ec:      mov x0, x21
1002e41f0:      bl  0x1008e35a8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1002e41f4:      mov x9, x0
1002e41f8:      mov x8, #0x0                ; =0
1002e41fc:      mov x22, #0x0               ; =0
1002e4200:      mov x0, x20
1002e4204:      tbnz    w9, #0x0, 0x1002e4304 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x34c>
1002e4208:      b   0x1002e4418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1002e420c:      mov x24, x0
1002e4210:      mov x8, x0
1002e4214:      cmp w9, #0x1
1002e4218:      b.eq    0x1002e4300 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x348>
1002e421c:      cmp w9, #0x9
1002e4220:      b.ne    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e4224:      ldr w8, [x21, #0x4]
1002e4228:      mov w9, #0x5841             ; =22593
1002e422c:      movk    w9, #0x4c5a, lsl #16
1002e4230:      cmp w8, w9
1002e4234:      b.ne    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e4238:      mov x0, x21
1002e423c:      bl  0x1002ac118 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime9json_tape22force_materialize_lazy>
1002e4240:      mov x22, x0
1002e4244:      mov x0, x20
1002e4248:      cbnz    x22, 0x1002e43d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1002e424c:      b   0x1002e4418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1002e4250:      ldr w8, [x21]
1002e4254:      mov w9, #0xe100             ; =57600
1002e4258:      movk    w9, #0x5f5, lsl #16
1002e425c:      orr w9, w9, #0x1
1002e4260:      cmp w8, w9
1002e4264:      b.hs    0x1002e4130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1002e4268:      ldr w9, [x21, #0x4]
1002e426c:      cmp w8, w9
1002e4270:      b.hi    0x1002e4130 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x178>
1002e4274:      mov x22, x21
1002e4278:      b   0x1002e43d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1002e427c:      mov x24, x0
1002e4280:      ldr x21, [x0, #0x8]
1002e4284:      mov x0, x21
1002e4288:      bl  0x1002d386c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1002e428c:      cbz x0, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e4290:      mov x8, x0
1002e4294:      ldrb    w9, [x0]
1002e4298:      cmp w9, #0x1
1002e429c:      b.ne    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e42a0:      ldrsb   w9, [x8, #0x1]
1002e42a4:      tbz w9, #0x1f, 0x1002e4154 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x19c>
1002e42a8:      mov w25, #0x1               ; =1
1002e42ac:      ldr x21, [x8, #0x8]
1002e42b0:      mov x0, x21
1002e42b4:      bl  0x1002d386c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5value10addr_class26try_read_tracked_gc_header>
1002e42b8:      cbz x0, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e42bc:      mov x8, x0
1002e42c0:      mov x22, #0x0               ; =0
1002e42c4:      ldrb    w9, [x0]
1002e42c8:      cmp w9, #0x1
1002e42cc:      b.ne    0x1002e4418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1002e42d0:      cmp w25, #0x3f
1002e42d4:      b.hi    0x1002e4418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
1002e42d8:      add w25, w25, #0x1
1002e42dc:      ldrsb   w9, [x8, #0x1]
1002e42e0:      tbnz    w9, #0x1f, 0x1002e42ac <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x2f4>
1002e42e4:      str x21, [x24, #0x8]
1002e42e8:      ldrb    w10, [x24, #0x1]
1002e42ec:      orr w10, w10, #0x80
1002e42f0:      strb    w10, [x24, #0x1]
1002e42f4:      ldrb    w9, [x8]
1002e42f8:      cmp w9, #0x1
1002e42fc:      b.ne    0x1002e421c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x264>
1002e4300:      mov x0, x20
1002e4304:      ldp w10, w9, [x21]
1002e4308:      cmp w10, w9
1002e430c:      b.ls    0x1002e4330 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x378>
1002e4310:      cbz x24, 0x1002e4344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
1002e4314:      ldr w8, [x8, #0x4]
1002e4318:      ubfiz   x9, x9, #3, #32
1002e431c:      add x9, x9, #0x10
1002e4320:      mov x22, x21
1002e4324:      cmp x9, x8
1002e4328:      b.eq    0x1002e43d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1002e432c:      b   0x1002e4344 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x38c>
1002e4330:      mov w8, #0xe100             ; =57600
1002e4334:      movk    w8, #0x5f5, lsl #16
1002e4338:      mov x22, x21
1002e433c:      cmp w10, w8
1002e4340:      b.ls    0x1002e43d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1002e4344:      adrp    x8, 0x10117c000 <_out_buf+0x3dc8>
1002e4348:      add x8, x8, #0x814
1002e434c:      ldaprb  w8, [x8]
1002e4350:      cbz w8, 0x1002e438c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1002e4354:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e4358:      add x8, x8, #0x238
1002e435c:      ldapr   x9, [x8]
1002e4360:      cmp x9, x21
1002e4364:      b.hi    0x1002e438c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1002e4368:      ldapur  x8, [x8, #0x8]
1002e436c:      cmp x8, x21
1002e4370:      b.lo    0x1002e438c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1002e4374:      mov x0, x21
1002e4378:      bl  0x100192b9c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6buffer6header25is_registered_buffer_slow>
1002e437c:      tbz w0, #0x0, 0x1002e438c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x3d4>
1002e4380:      mov x0, x20
1002e4384:      mov x22, x21
1002e4388:      b   0x1002e43d4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x41c>
1002e438c:      adrp    x8, 0x1011fd000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime6object22native_module_registry16NM_CTOR_REGISTRY+0x138>
1002e4390:      add x8, x8, #0xb78
1002e4394:      ldaprb  w8, [x8]
1002e4398:      cbz w8, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e439c:      adrp    x8, 0x101131000 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime5timer16TIMER_REF_STATES+0x28>
1002e43a0:      add x8, x8, #0xd88
1002e43a4:      ldapr   x9, [x8]
1002e43a8:      cmp x21, x9
1002e43ac:      b.lo    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e43b0:      ldapur  x8, [x8, #0x8]
1002e43b4:      cmp x21, x8
1002e43b8:      b.hi    0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e43bc:      mov x0, x21
1002e43c0:      bl  0x1008e35a8 <__RNvNtCs5gMwpk3Cs4e_13perry_runtime10typedarray34lookup_registered_typed_array_kind>
1002e43c4:      mov x8, x0
1002e43c8:      mov x0, x20
1002e43cc:      mov x22, x21
1002e43d0:      tbz w8, #0x0, 0x1002e4414 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x45c>
1002e43d4:      cmp x22, x23
1002e43d8:      b.eq    0x1002e44b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1002e43dc:      str x22, [x0, #0x20]
1002e43e0:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e43e4:      add x8, x8, #0x910
1002e43e8:      ldapr   x8, [x8]
1002e43ec:      cbnz    x8, 0x1002e4454 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x49c>
1002e43f0:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e43f4:      ldrb    w8, [x8, #0x918]
1002e43f8:      tbz w8, #0x0, 0x1002e4470 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4b8>
1002e43fc:      mov x0, x20
1002e4400:      mov x1, x19
1002e4404:      mov x2, x22
1002e4408:      mov w3, #0x0                ; =0
1002e440c:      bl  0x1004357e8 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier26write_barrier_slot_decoded>
1002e4410:      b   0x1002e4488 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d0>
1002e4414:      mov x22, #0x0               ; =0
1002e4418:      mov x0, x22
1002e441c:      ldp x29, x30, [sp, #0x40]
1002e4420:      ldp x20, x19, [sp, #0x30]
1002e4424:      ldp x22, x21, [sp, #0x20]
1002e4428:      ldp x24, x23, [sp, #0x10]
1002e442c:      ldp x26, x25, [sp], #0x50
1002e4430:      ret
1002e4434:      bl  0x100ccaa2c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime7tls_hot12hot_uncached>
1002e4438:      mov x8, x0
1002e443c:      mov x0, x20
1002e4440:      lsr x1, x21, #20
1002e4444:      ldr x8, [x8, #0x10]
1002e4448:      ldrb    w9, [x8, #0x28]
1002e444c:      tbnz    w9, #0x0, 0x1002e4058 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xa0>
1002e4450:      b   0x1002e4074 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0xbc>
1002e4454:      adrp    x0, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e4458:      add x0, x0, #0x910
1002e445c:      bl  0x100cc5d84 <__RINvMNtNtCs8BpVhDwHqJW_3std4sync9once_lockINtB3_8OnceLockbE10initializeNCINvB2_11get_or_initNCNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier22write_barriers_enabled0E0zEB1A_>
1002e4460:      mov x0, x20
1002e4464:      adrp    x8, 0x101130000 <_perry_global_baseline_worker_ts__1>
1002e4468:      ldrb    w8, [x8, #0x918]
1002e446c:      tbnz    w8, #0x0, 0x1002e43fc <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x444>
1002e4470:      adrp    x8, 0x1011fc000 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime5array8subclass20DENSE_SUBCLASS_CACHE+0x7f7e0>
1002e4474:      add x8, x8, #0xb70
1002e4478:      ldr w8, [x8]
1002e447c:      cbz w8, 0x1002e448c <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4d4>
1002e4480:      mov x0, x22
1002e4484:      bl  0x1004369e4 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime2gc7barrier37incremental_mark_barrier_value_active>
1002e4488:      mov x0, x20
1002e448c:      ldr x8, [x19]
1002e4490:      cbz x8, 0x1002e44b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1002e4494:      ldr x8, [x0, #0x10]
1002e4498:      cbz x8, 0x1002e44b0 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x4f8>
1002e449c:      mov x0, x20
1002e44a0:      bl  0x10034b82c <__RNvNtCs5gMwpk3Cs4e_13perry_runtime15json_tape_store7release>
1002e44a4:      mov x0, x20
1002e44a8:      str xzr, [x20, #0x10]
1002e44ac:      str wzr, [x20, #0xc]
1002e44b0:      ldr w8, [x22]
1002e44b4:      str w8, [x0]
1002e44b8:      b   0x1002e4418 <__RNvNtNtCs5gMwpk3Cs4e_13perry_runtime9json_tape8mutation26resolve_materialized_array+0x460>
