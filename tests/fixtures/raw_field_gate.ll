; Excerpts from CI 40064647833 raw_numeric_object_fields (main36); text-check fixture, not linked IR.
define double @perry_fn_raw_numeric_object_fields_ts__forceDynamicRead(double %arg0) inlinehint optsize "frame-pointer"="non-leaf" "perry-nanbox-operands" gc "statepoint-example" {
entry.0:
  %r1 = alloca ptr addrspace(1)
  store ptr addrspace(1) null, ptr %r1
  %rs4gc.b1 = bitcast double %arg0 to i64
  %rs4gc.s1 = inttoptr i64 %rs4gc.b1 to ptr addrspace(1)
  store ptr addrspace(1) %rs4gc.s1, ptr %r1
  %r2 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 16
  %r3 = load ptr, ptr %r2
  %r4 = call i64 @llvm.read_register.i64(metadata !{!"rsp"})
  %r5 = ptrtoint ptr %r3 to i64
  %r6 = icmp ult i64 %r4, %r5
  br i1 %r6, label %stack_guard.overflow.2, label %stack_guard.ok.1

stack_guard.ok.1:
  %r7.rs4p = load ptr addrspace(1), ptr %r1
  %r7.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r7.rs4p) "gc-leaf-function"
  %r7 = bitcast i64 %r7.rs4o to double
  %r8 = bitcast double %r7 to i64
  %r9 = sub i64 %r8, 9222527611926740992
  %r10 = icmp ult i64 %r9, 281474974613504
  br i1 %r10, label %l2c.word.3, label %l2c.miss.6

stack_guard.overflow.2:
  call void @js_stack_overflow()
  unreachable

l2c.word.3:
  %r11 = inttoptr i64 %r9 to ptr
  %r12 = getelementptr i8, ptr %r11, i64 2097156
  %r13 = load i32, ptr %r12
  %r14 = and i32 %r13, -1073741825
  %r15 = sub i32 %r14, -2147483648
  %r16 = zext i32 %r15 to i64
  %r17 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0
  %r18 = load ptr, ptr %r17
  %r19 = getelementptr i8, ptr %r18, i64 24
  %r20 = load ptr, ptr %r19
  %r21 = getelementptr i8, ptr %r18, i64 32
  %r22 = load i64, ptr %r21
  %r23 = icmp ult i64 %r16, %r22
  br i1 %r23, label %l2c.record.8, label %l2c.miss.6

l2c.load.4:
  %r30 = inttoptr i64 %r9 to ptr
  %r31 = getelementptr i8, ptr %r30, i64 2097168
  %r32 = load i64, ptr %r31
  %r33 = icmp eq i64 %r32, 9222246136947933200
  br i1 %r33, label %l2c.miss.6, label %l2c.hit.5

l2c.hit.5:
  %r34 = bitcast i64 %r32 to double
  br label %l2c.merge.7

l2c.miss.6:
  %r35 = bitcast double %r7 to i64
  %r36 = sub i64 %r35, 9222527611926740992
  %r37 = icmp ult i64 %r36, 281474974613504
  %r38 = lshr i64 %r35, 48
  br i1 %r37, label %pget.recv_ok.9, label %pget.recv_other.11

l2c.merge.7:
  %r51 = phi double [ %r34, %l2c.hit.5 ], [ %r50, %pget.recv_merge.12 ]
  ret double %r51

l2c.record.8:
  %r24 = mul i64 %r16, 120
  %r25 = getelementptr i8, ptr %r20, i64 %r24
  %r26 = getelementptr i8, ptr %r25, i64 88
  %r27 = load i64, ptr %r26
  %r28 = and i64 %r27, 2
  %r29 = icmp ne i64 %r28, 0
  br i1 %r29, label %l2c.load.4, label %l2c.miss.6

pget.recv_ok.9:
  %r39 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0
  %r40 = load ptr, ptr %r39
  %r41 = load double, ptr @raw_numeric_object_fields_ts_.str.0.handle
  %r42 = bitcast double %r41 to i64
  %r43 = call double @perry_probe_get(ptr %r40, i64 %r36, i64 %r42, i64 8999596768310594794)
  %r44 = bitcast double %r43 to i64
  %r45 = icmp ne i64 %r44, 9222246136947933200
  br i1 %r45, label %pget.recv_merge.12, label %pget.slow.10

pget.slow.10:
  %r46 = load double, ptr @raw_numeric_object_fields_ts_.str.0.handle
  %r47 = bitcast double %r46 to i64
  %r48.image = getelementptr i8, ptr @perry_image_literal_0_76616c7565, i64 8
  %r48 = ptrtoint ptr %r48.image to i64
  %r52.rs4p = load ptr addrspace(1), ptr %r1
  %r52.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r52.rs4p) "gc-leaf-function"
  %r52 = bitcast i64 %r52.rs4o to double
  %r53 = bitcast double %r52 to i64
  %r49 = call double @js_object_get_field_value(i64 %r53, i64 %r48, i64 982026375101677568)
  br label %pget.recv_merge.12

pget.recv_other.11:
  br label %pget.slow.10

pget.recv_merge.12:
  %r50 = phi double [ %r43, %pget.recv_ok.9 ], [ %r49, %pget.slow.10 ]
  br label %l2c.merge.7
}

define double @perry_fn_raw_numeric_object_fields_ts__forceDynamicWrite(double %arg1, double %arg2) inlinehint optsize "frame-pointer"="non-leaf" "perry-nanbox-operands" gc "statepoint-example" {
entry.0:
  %r1 = alloca ptr addrspace(1)
  store ptr addrspace(1) null, ptr %r1
  %rs4gc.b1 = bitcast double %arg1 to i64
  %rs4gc.s1 = inttoptr i64 %rs4gc.b1 to ptr addrspace(1)
  store ptr addrspace(1) %rs4gc.s1, ptr %r1
  %r2 = alloca ptr addrspace(1)
  store ptr addrspace(1) null, ptr %r2
  %rs4gc.b2 = bitcast double %arg2 to i64
  %rs4gc.s2 = inttoptr i64 %rs4gc.b2 to ptr addrspace(1)
  store ptr addrspace(1) %rs4gc.s2, ptr %r2
  %r3 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 16
  %r4 = load ptr, ptr %r3
  %r5 = call i64 @llvm.read_register.i64(metadata !{!"rsp"})
  %r6 = ptrtoint ptr %r4 to i64
  %r7 = icmp ult i64 %r5, %r6
  br i1 %r7, label %stack_guard.overflow.2, label %stack_guard.ok.1

stack_guard.ok.1:
  %r8.rs4p = load ptr addrspace(1), ptr %r1
  %r8.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r8.rs4p) "gc-leaf-function"
  %r8 = bitcast i64 %r8.rs4o to double
  %r9.rs4p = load ptr addrspace(1), ptr %r2
  %r9.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r9.rs4p) "gc-leaf-function"
  %r9 = bitcast i64 %r9.rs4o to double
  %r10 = bitcast double %r9 to i64
  %r11 = bitcast double %r8 to i64
  %r12 = bitcast double %r8 to i64
  %r13 = sub i64 %r12, 9222527611926740992
  %r14 = icmp ult i64 %r13, 281474974613504
  br i1 %r14, label %l2s.word.3, label %l2s.miss.7

stack_guard.overflow.2:
  call void @js_stack_overflow()
  unreachable

l2s.word.3:
  %r15 = inttoptr i64 %r13 to ptr
  %r16 = getelementptr i8, ptr %r15, i64 2097156
  %r17 = load i32, ptr %r16
  %r18 = and i32 %r17, -1073741825
  %r19 = sub i32 %r18, -2147483648
  %r20 = zext i32 %r19 to i64
  %r21 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0
  %r22 = load ptr, ptr %r21
  %r23 = getelementptr i8, ptr %r22, i64 24
  %r24 = load ptr, ptr %r23
  %r25 = getelementptr i8, ptr %r22, i64 32
  %r26 = load i64, ptr %r25
  %r27 = icmp ult i64 %r20, %r26
  br i1 %r27, label %l2c.record.9, label %l2s.miss.7

l2s.meta.4:
  %r42 = inttoptr i64 %r13 to ptr
  %r43 = getelementptr i8, ptr %r42, i64 2097160
  %r44 = load i64, ptr %r43
  %r45 = icmp eq i64 %r44, 0
  br i1 %r45, label %l2s.load.5, label %l2s.miss.7

l2s.load.5:
  %r46 = inttoptr i64 %r13 to ptr
  %r47 = getelementptr i8, ptr %r46, i64 2097168
  %r48 = load i64, ptr %r47
  %r49 = icmp eq i64 %r48, 9222246136947933200
  br i1 %r49, label %l2s.miss.7, label %l2s.hit.6

l2s.hit.6:
  store double %r9, ptr %r47
  %r50 = add i64 %r13, 2097152
  %r51 = bitcast double %r9 to i64
  %r52 = and i64 %r51, 9223372036854775807
  %r53 = icmp uge i64 %r52, 9221401712017801216
  %r54 = icmp ult i64 %r51, 281474976710656
  %r55 = or i1 %r53, %r54
  br i1 %r55, label %l2s.classify.14, label %l2s.gc_bookkeeping.done.13

l2s.miss.7:
  %r77 = getelementptr i8, ptr @PERRY_AGENT_PTRS, i64 0
  %r78 = load ptr, ptr %r77
  %r79 = load double, ptr @raw_numeric_object_fields_ts_.str.0.handle
  %r80 = bitcast double %r79 to i64
  %r81 = bitcast double %r8 to i64
  %r82 = call i32 @perry_probe_set(ptr %r78, i64 %r81, i64 %r80, i64 8999596768310594794, double %r9)
  %r83 = icmp ne i32 %r82, 0
  br i1 %r83, label %put.pic.merge.20, label %put.pic.set.19

l2s.merge.8:
  %r87 = phi double [ %r9, %l2s.gc_bookkeeping.done.13 ], [ %r86, %put.pic.merge.20 ]
  ret double 0x7FFC000000000001

l2c.record.9:
  %r28 = mul i64 %r20, 120
  %r29 = getelementptr i8, ptr %r24, i64 %r28
  %r30 = getelementptr i8, ptr %r29, i64 104
  %r31 = load i64, ptr %r30
  %r32 = and i64 %r31, 2
  %r33 = icmp ne i64 %r32, 0
  br i1 %r33, label %l2s.lane.10, label %l2s.miss.7

l2s.lane.10:
  %r34 = bitcast double %r9 to i64
  %r35 = and i64 %r34, 9218868437227405312
  %r36 = icmp ne i64 %r35, 9218868437227405312
  br i1 %r36, label %l2s.meta.4, label %l2s.rep.11

l2s.rep.11:
  %r37 = getelementptr i8, ptr %r29, i64 48
  %r38 = load i64, ptr %r37
  %r39 = lshr i64 %r38, 0
  %r40 = and i64 %r39, 3
  %r41 = icmp eq i64 %r40, 0
  br i1 %r41, label %l2s.meta.4, label %l2s.miss.7

l2s.scalar.tagged.12:
  br label %l2s.gc_bookkeeping.done.13

l2s.gc_bookkeeping.done.13:
  br label %l2s.merge.8

l2s.classify.14:
  %r56 = lshr i64 %r51, 48
  %r57 = icmp eq i64 %r56, 0
  %r58 = icmp uge i64 %r51, 4096
  %r59 = and i1 %r57, %r58
  %r60 = icmp eq i64 %r56, 32765
  %r61 = icmp eq i64 %r56, 32767
  %r62 = icmp eq i64 %r56, 32762
  %r63 = or i1 %r60, %r61
  %r64 = or i1 %r63, %r62
  %r65 = or i1 %r64, %r59
  br i1 %r65, label %l2s.gc_bookkeeping.15, label %l2s.scalar.tagged.12

l2s.gc_bookkeeping.15:
  %r66 = lshr i64 %r51, 48
  %r67 = icmp eq i64 %r66, 32767
  br i1 %r67, label %l2s.string_alias.16, label %l2s.book_done.17

l2s.string_alias.16:
  call void @js_string_addref_if_heap_string(double %r9) "gc-leaf-function"
  br label %l2s.book_done.17

l2s.book_done.17:
  %r68 = sub i64 %r50, 7
  %r69 = inttoptr i64 %r68 to ptr
  %r70 = load i8, ptr %r69
  %r71 = and i8 %r70, 34
  %r72 = icmp ne i8 %r71, 2
  %r73 = load atomic i32, ptr @PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT monotonic, align 4
  %r74 = icmp ne i32 %r73, 0
  %r75 = or i1 %r72, %r74
  br i1 %r75, label %l2s.barrier.18, label %l2s.gc_bookkeeping.done.13

l2s.barrier.18:
  %r76 = ptrtoint ptr %r47 to i64
  %r95.rs4p = load ptr addrspace(1), ptr %r2
  %r95.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r95.rs4p) "gc-leaf-function"
  %r95 = bitcast i64 %r95.rs4o to double
  %r96 = bitcast double %r95 to i64
  call void @js_write_barrier_slot_validated_parent(i64 %r50, i64 %r76, i64 %r96)
  br label %l2s.gc_bookkeeping.done.13

put.pic.set.19:
  %r93 = load double, ptr @raw_numeric_object_fields_ts_.str.0.handle
  %r94 = bitcast double %r93 to i64
  %r84 = bitcast i64 %r94 to double
  %r88.rs4p = load ptr addrspace(1), ptr %r1
  %r88.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r88.rs4p) "gc-leaf-function"
  %r88 = bitcast i64 %r88.rs4o to double
  %r89.rs4p = load ptr addrspace(1), ptr %r2
  %r89.rs4o = call i64 asm "", "=r,0"(ptr addrspace(1) %r89.rs4p) "gc-leaf-function"
  %r89 = bitcast i64 %r89.rs4o to double
  %r90 = load double, ptr @raw_numeric_object_fields_ts_.str.0.handle
  %r91 = bitcast double %r90 to i64
  %r92 = bitcast i64 %r91 to double
  %r85 = call double @js_put_value_set(double %r88, double %r92, double %r89, double %r88, i32 1)
  br label %put.pic.merge.20

put.pic.merge.20:
  %r86 = phi double [ %r9, %l2s.miss.7 ], [ %r85, %put.pic.set.19 ]
  br label %l2s.merge.8
}

define double @perry_fn_raw_numeric_object_fields_ts__rawNumericObjectFieldsChecksum() {
class_family.word.64:
  %r261 = inttoptr i64 %r259 to ptr
  %r262 = getelementptr i8, ptr %r261, i64 2097156
  %r263 = load i32, ptr %r262
  %r264 = getelementptr i8, ptr @perry_layout_class_1, i64 12
  %r265 = load i32, ptr %r264
  %r266 = sub i32 %r265, -2147483648
  %r267 = icmp ult i32 %r266, 1073741824
  %r268 = icmp eq i32 %r263, %r265
  %r269 = and i1 %r267, %r268
  %r270 = or i1 false, %r269
  br i1 %r270, label %class_family.load.67, label %class_family.next.68

class_family.miss.65:
  %r276 = bitcast double %r257 to i64
  %r277 = sub i64 %r276, 9222527611926740992
  %r278 = icmp ult i64 %r277, 281474974613504
  br i1 %r278, label %l2c.word.70, label %l2c.miss.73

class_family.merge.66:
  %r320 = phi double [ %r275, %class_family.present.69 ], [ %r319, %l2c.merge.74 ]
  %r321 = bitcast double %r320 to i64
  %r322 = lshr i64 %r321, 48
  %r323 = icmp eq i64 %r322, 32767
  %r324 = icmp eq i64 %r322, 32761
  %r325 = or i1 %r323, %r324
  %r326 = select i1 %r325, i64 9222246136947933188, i64 9222246136947933187
  %r327 = bitcast i64 %r326 to double
  %r328 = icmp eq i64 %r326, 9222246136947933188
  br i1 %r328, label %ternary.then.80, label %ternary.else.81

class_family.load.67:
  %r271 = inttoptr i64 %r259 to ptr
  %r272 = getelementptr i8, ptr %r271, i64 2097168
  %r273 = load i64, ptr %r272
  %r274 = icmp ne i64 %r273, 9222246136947933200
  br i1 %r274, label %class_family.present.69, label %class_family.miss.65

class_family.next.68:
  br label %class_family.miss.65

class_family.present.69:
  %r275 = bitcast i64 %r273 to double
  br label %class_family.merge.66

}
