; PR #13337: Storage glob/dist/commonjs/index.min.js __620.
; LLVM issue #194184; fixed by 5c9647b8e680 in LLVM 22.1.5.
; Retain the extracted optimized invoke/catchpad CFG.
; ModuleID = '/root/WEH4/mac-glob-opt.bc'
source_filename = "perry_module"
target datalayout = "e-m:e-p:32:32-p10:8:8-p20:8:8-i64:64-i128:128-n32:64-S128-ni:1:10:20"
target triple = "wasm32-unknown-wasip2"

@node_modules_glob_dist_commonjs_index_min_js_.str.298.bytes = external hidden unnamed_addr constant [5 x i8]
@node_modules_glob_dist_commonjs_index_min_js_.str.316.bytes = external hidden unnamed_addr constant [4 x i8]
@PERRY_GC_POLL_ARMED = external global i32
@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED = external global i8
@PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT = external local_unnamed_addr global i32
@node_modules_glob_dist_commonjs_index_min_js_.str.124.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.140.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.183.handle = external hidden thread_local unnamed_addr global double
@node_modules_glob_dist_commonjs_index_min_js_.str.243.handle = external hidden thread_local unnamed_addr global double
@node_modules_glob_dist_commonjs_index_min_js_.str.251.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.278.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.298.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.311.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.312.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.317.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@node_modules_glob_dist_commonjs_index_min_js_.str.318.dispatch = external hidden unnamed_addr constant { i32, i32, i64, ptr }
@perry_image_literal_0_666c6f77696e67 = external constant { i8, i8, i16, i32, i32, i32, i32, i32, i32, [8 x i8], i64 }, section "perry_strings", align 8
@node_modules_glob_dist_commonjs_index_min_js_.str.319.handle = external hidden thread_local unnamed_addr global double
@node_modules_glob_dist_commonjs_index_min_js_.str.320.handle = external hidden thread_local unnamed_addr global double
@node_modules_glob_dist_commonjs_index_min_js_.str.321.handle = external hidden thread_local unnamed_addr global double
@"perry_closure_node_modules_glob_dist_commonjs_index_min_js__621$info" = external constant { { ptr, i16, i16, i32, i32, i32, ptr, i64, ptr, i32, i16, i16, i64 }, i32, i32, ptr }
@"perry_closure_node_modules_glob_dist_commonjs_index_min_js__622$info" = external constant { { ptr, i16, i16, i32, i32, i32, ptr, i64, ptr, i32, i16, i16, i64 }, i32, i32, ptr }

; Function Attrs: mustprogress nofree willreturn memory(read)
declare i32 @js_is_truthy(double) local_unnamed_addr #0

declare ptr @js_closure_alloc_empty(ptr, i32) local_unnamed_addr #1

declare ptr @js_closure_alloc_small(ptr, i64, i64, i64, i64) local_unnamed_addr #1

declare i64 @js_closure_get_capture_bits(ptr, i32) local_unnamed_addr #1

declare i64 @js_closure_unbox_callee_checked(double) local_unnamed_addr #1

declare double @js_closure_call1(ptr, i64, double) local_unnamed_addr #1

declare i64 @js_box_get_bits_trusted_named(ptr, double) local_unnamed_addr #1

declare void @js_string_addref_if_heap_string(double) local_unnamed_addr #1

declare double @js_to_numeric(double) local_unnamed_addr #1

declare double @js_numeric_step(double, i32) local_unnamed_addr #1

declare double @js_builtin_namespace_value(i64) local_unnamed_addr #1

declare double @js_builtin_namespace_member(double, ptr, i32) local_unnamed_addr #1

declare void @js_throw(double) local_unnamed_addr #1

declare void @js_wasm_eh_checkpoint_save(ptr) local_unnamed_addr #1

declare void @js_wasm_eh_pad_enter(ptr) local_unnamed_addr #1

declare i1 @js_wasm_eh_is_perry(ptr) local_unnamed_addr #1

; Function Attrs: nocallback nofree nosync nounwind willreturn
declare ptr @llvm.wasm.get.exception(token) #2

; Function Attrs: noreturn
declare void @llvm.wasm.rethrow() #3

declare i32 @__gxx_wasm_personality_v0(...) #1

declare double @js_get_exception() local_unnamed_addr #1

declare double @js_catch_enter() local_unnamed_addr #1

declare double @js_native_call_method_patched_proto(double, ptr, i32, ptr, i32) local_unnamed_addr #1

declare ptr @js_promise_all_iterable(double) local_unnamed_addr #1

declare i32 @perry_array_element_kind(ptr) local_unnamed_addr #1

declare ptr @js_array_alloc_literal(i32) local_unnamed_addr #1

declare i64 @js_array_push_f64_spec_or_own(ptr, double, ptr) local_unnamed_addr #1

declare i32 @js_array_mark_numeric_f64_layout(ptr) local_unnamed_addr #1

declare i32 @js_array_length(ptr) local_unnamed_addr #1

declare i64 @js_array_length_leaf(ptr) local_unnamed_addr #1

declare double @js_value_length_property_f64(double) local_unnamed_addr #1

declare ptr @js_shadow_frame_enter(i32) local_unnamed_addr #1

declare void @js_shadow_frame_pop(i64) local_unnamed_addr #1

declare void @js_shadow_slot_set(i32, i64) local_unnamed_addr #1

declare void @js_shadow_slot_bind(i32, ptr) local_unnamed_addr #1

declare void @js_gc_loop_safepoint() local_unnamed_addr #1

declare void @js_write_barrier_slot_validated_parent(i64, i64, i64) local_unnamed_addr #1

declare double @js_array_record_stack_dispatch(ptr, i32) local_unnamed_addr #1

declare i32 @js_array_record_stack_enter(ptr, i32, i32) local_unnamed_addr #1

declare ptr @perry_intrinsic_proto(i32) local_unnamed_addr #1

declare double @js_typed_feedback_native_call_method_by_id(i64, double, i64, ptr, i32) local_unnamed_addr #1

; Function Attrs: cold
declare double @js_object_get_field_value(i64, ptr, i64) local_unnamed_addr #4

declare ptr @perry_shape_dir_cell() local_unnamed_addr #1

declare double @perry_probe_get(ptr, i64, i64, i64) local_unnamed_addr #1

declare double @js_method_prepare(double, i64) local_unnamed_addr #1

declare double @js_method_call_value(double, double, ptr, i32) local_unnamed_addr #1

; Function Attrs: optsize
declare noundef double @perry_closure_node_modules_glob_dist_commonjs_index_min_js__619(ptr, i64) #5

; Function Attrs: optsize
define double @perry_closure_node_modules_glob_dist_commonjs_index_min_js__620(ptr %this_closure.ptr, i64 %js_this, double %arg8933, double %arg8934, double %arg8935) #5 personality ptr @__gxx_wasm_personality_v0 {
entry.0:
  %0 = ptrtoint ptr %this_closure.ptr to i32
  %this_closure = zext i32 %0 to i64
  %r3 = tail call ptr @js_shadow_frame_enter(i32 48)
  %r4 = getelementptr inbounds nuw i8, ptr %r3, i32 12
  %r5.w = load i32, ptr %r4, align 4
  %r15 = alloca double, align 8
  %r21 = alloca [6 x double], align 8
  %r23 = getelementptr inbounds nuw i8, ptr %r21, i32 8
  %r24 = getelementptr inbounds nuw i8, ptr %r21, i32 16
  %r25 = getelementptr inbounds nuw i8, ptr %r21, i32 24
  %r26 = getelementptr inbounds nuw i8, ptr %r21, i32 32
  %r27 = getelementptr inbounds nuw i8, ptr %r21, i32 40
  %r28 = alloca [6 x double], align 8
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(48) %r21, i8 0, i64 48, i1 false)
  %r30 = getelementptr inbounds nuw i8, ptr %r28, i32 8
  %r31 = getelementptr inbounds nuw i8, ptr %r28, i32 16
  %r32 = getelementptr inbounds nuw i8, ptr %r28, i32 24
  %r33 = getelementptr inbounds nuw i8, ptr %r28, i32 32
  %r34 = getelementptr inbounds nuw i8, ptr %r28, i32 40
  %r67 = alloca double, align 8
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(48) %r28, i8 0, i64 48, i1 false)
  store double 0.000000e+00, ptr %r67, align 8
  %r73 = alloca double, align 8
  store double 0.000000e+00, ptr %r73, align 8
  %r79 = alloca [2 x double], align 8
  %r121 = alloca [6 x double], align 8
  %r123 = getelementptr inbounds nuw i8, ptr %r121, i32 8
  %r124 = getelementptr inbounds nuw i8, ptr %r121, i32 16
  %r125 = getelementptr inbounds nuw i8, ptr %r121, i32 24
  %r126 = getelementptr inbounds nuw i8, ptr %r121, i32 32
  %r127 = getelementptr inbounds nuw i8, ptr %r121, i32 40
  %r128 = alloca double, align 8
  call void @llvm.memset.p0.i64(ptr noundef nonnull align 8 dereferenceable(48) %r121, i8 0, i64 48, i1 false)
  store double 0x7FFC000000000001, ptr %r128, align 8
  %r149 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r149, align 8
  %r151 = alloca [120 x i8], align 8
  %r225 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r225, align 8
  %r285 = alloca [1 x double], align 8
  %r297 = alloca i32, align 4
  %r322 = alloca double, align 8
  %r323 = alloca [120 x i8], align 8
  %r356 = alloca double, align 8
  %r438 = alloca [1 x double], align 8
  %r470 = alloca [1 x double], align 8
  %r487 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r487, align 8
  %r489 = alloca [120 x i8], align 8
  %r563 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r563, align 8
  %r685 = alloca [1 x double], align 8
  %r743 = alloca double, align 8
  %r744 = alloca [120 x i8], align 8
  %r777 = alloca double, align 8
  %r886 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r886, align 8
  %r888 = alloca [120 x i8], align 8
  %r962 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r962, align 8
  %r970 = alloca double, align 8
  store double 0x7FFC000000000001, ptr %r970, align 8
  %r1012 = alloca [2 x double], align 8
  %r1045 = alloca i32, align 4
  %r1095 = alloca double, align 8
  %r1096 = alloca [120 x i8], align 8
  %r1129 = alloca double, align 8
  %r1267 = alloca [2 x double], align 8
  call void @js_shadow_slot_bind(i32 25, ptr nonnull %r21)
  call void @js_shadow_slot_bind(i32 26, ptr nonnull %r23)
  call void @js_shadow_slot_bind(i32 27, ptr nonnull %r24)
  call void @js_shadow_slot_bind(i32 28, ptr nonnull %r25)
  call void @js_shadow_slot_bind(i32 29, ptr nonnull %r26)
  call void @js_shadow_slot_bind(i32 30, ptr nonnull %r27)
  call void @js_shadow_slot_bind(i32 31, ptr nonnull %r28)
  call void @js_shadow_slot_bind(i32 32, ptr nonnull %r30)
  call void @js_shadow_slot_bind(i32 33, ptr nonnull %r31)
  call void @js_shadow_slot_bind(i32 34, ptr nonnull %r32)
  call void @js_shadow_slot_bind(i32 35, ptr nonnull %r33)
  call void @js_shadow_slot_bind(i32 36, ptr nonnull %r34)
  call void @js_shadow_slot_bind(i32 39, ptr nonnull %r121)
  call void @js_shadow_slot_bind(i32 40, ptr nonnull %r123)
  call void @js_shadow_slot_bind(i32 41, ptr nonnull %r124)
  call void @js_shadow_slot_bind(i32 42, ptr nonnull %r125)
  call void @js_shadow_slot_bind(i32 43, ptr nonnull %r126)
  call void @js_shadow_slot_bind(i32 44, ptr nonnull %r127)
  %r7 = alloca double, align 8
  store double %arg8933, ptr %r7, align 8
  call void @js_shadow_slot_bind(i32 0, ptr nonnull %r7)
  %r8 = alloca double, align 8
  store double %arg8934, ptr %r8, align 8
  call void @js_shadow_slot_bind(i32 1, ptr nonnull %r8)
  %r9 = alloca double, align 8
  store double %arg8935, ptr %r9, align 8
  call void @js_shadow_slot_bind(i32 2, ptr nonnull %r9)
  %r10 = alloca i64, align 8
  store i64 %this_closure, ptr %r10, align 8
  call void @js_shadow_slot_bind(i32 23, ptr nonnull %r10)
  %r12 = getelementptr i8, ptr %this_closure.ptr, i32 16
  %r145 = load double, ptr %r12, align 8
  store double %r145, ptr %r15, align 8
  call void @js_shadow_slot_bind(i32 24, ptr nonnull %r15)
  %r357 = load i64, ptr %r9, align 8
  %r38 = icmp eq i64 %r357, 9222246136947933185
  br i1 %r38, label %if.then.1, label %if.merge.3

if.then.1:                                        ; preds = %entry.0
  store double 0x7FFC000000000003, ptr %r9, align 8
  call void @js_shadow_slot_set(i32 2, i64 0)
  br label %if.merge.3

if.merge.3:                                       ; preds = %entry.0, %if.then.1
  %r46 = load double, ptr %r7, align 8
  %r47 = bitcast double %r46 to i64
  %r48 = and i64 %r47, 9221120237041090560
  %r49.not = icmp eq i64 %r48, 9221120237041090560
  br i1 %r49.not, label %truthy.tag.5, label %truthy.num.4

truthy.num.4:                                     ; preds = %if.merge.3
  %r50 = fcmp ueq double %r46, 0.000000e+00
  br i1 %r50, label %if.merge.11, label %if.then.9

truthy.tag.5:                                     ; preds = %if.merge.3
  %r52 = add i64 %r47, -9222246136947933185
  %r53 = icmp ult i64 %r52, 4
  br i1 %r53, label %truthy.merge.7, label %truthy.obj.8

truthy.slow.6:                                    ; preds = %truthy.obj.8
  %r59 = call i32 @js_is_truthy(double %r46)
  %r60.not = icmp eq i32 %r59, 0
  br i1 %r60.not, label %if.merge.11, label %if.then.9

truthy.merge.7:                                   ; preds = %truthy.tag.5
  %r51 = icmp eq i64 %r47, 9222246136947933188
  br i1 %r51, label %if.then.9, label %if.merge.11

truthy.obj.8:                                     ; preds = %truthy.tag.5
  %r54.mask = and i64 %r47, -281474976710656
  %r55 = icmp eq i64 %r54.mask, 9222527611924643840
  %r56 = and i64 %r47, 281474976710655
  %r57 = icmp ne i64 %r56, 0
  %r58 = and i1 %r55, %r57
  br i1 %r58, label %if.then.9, label %truthy.slow.6

if.then.9:                                        ; preds = %truthy.slow.6, %truthy.num.4, %truthy.obj.8, %truthy.merge.7
  %r62 = load i64, ptr %r10, align 8
  %1 = trunc i64 %r62 to i32
  %2 = add i32 %1, 56
  %r64 = inttoptr i32 %2 to ptr
  %r6520 = load double, ptr %r64, align 8
  store double %r6520, ptr %r67, align 8
  call void @js_shadow_slot_bind(i32 37, ptr nonnull %r67)
  %r72 = load double, ptr %r7, align 8
  store double %r72, ptr %r73, align 8
  call void @js_shadow_slot_bind(i32 38, ptr nonnull %r73)
  %r74 = load double, ptr %r67, align 8
  %r75 = load double, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.183.handle, align 8
  %r76 = load double, ptr %r73, align 8
  %r78 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.278.dispatch to i64), 9221120237041090560
  store double %r75, ptr %r79, align 8
  %r81 = getelementptr inbounds nuw i8, ptr %r79, i32 8
  store double %r76, ptr %r81, align 8
  %r.i = call double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347281, double %r74, i64 range(i64 9221120237041090560, 0) %r78, ptr nonnull %r79, i32 2)
  store double 0.000000e+00, ptr %r67, align 8
  call void @js_shadow_slot_set(i32 37, i64 0)
  store double 0.000000e+00, ptr %r73, align 8
  call void @js_shadow_slot_set(i32 38, i64 0)
  br label %common.ret

if.merge.11:                                      ; preds = %truthy.slow.6, %truthy.num.4, %truthy.merge.7
  %r84 = load i64, ptr %r10, align 8
  %3 = trunc i64 %r84 to i32
  %4 = add i32 %3, 32
  %r86 = inttoptr i32 %4 to ptr
  %r87 = load i64, ptr %r86, align 8
  %r88 = bitcast i64 %r87 to double
  %r89 = and i64 %r87, 9221120237041090560
  %r90.not = icmp eq i64 %r89, 9221120237041090560
  br i1 %r90.not, label %truthy.tag.15, label %truthy.num.14

common.ret:                                       ; preds = %lookup_first.merge.319, %trusted_box.read.330, %truthy.merge.325, %truthy.obj.326, %truthy.num.322, %truthy.slow.324, %ns_guard.merge.110, %if.then.9
  %common.ret.op = phi double [ %r.i, %if.then.9 ], [ 0x7FFC000000000001, %ns_guard.merge.110 ], [ 0x7FFC000000000001, %truthy.slow.324 ], [ 0x7FFC000000000001, %truthy.num.322 ], [ 0x7FFC000000000001, %truthy.obj.326 ], [ 0x7FFC000000000001, %truthy.merge.325 ], [ 0x7FFC000000000001, %trusted_box.read.330 ], [ 0x7FFC000000000001, %lookup_first.merge.319 ]
  %r5 = zext i32 %r5.w to i64
  %r6 = add nsw i64 %r5, -1
  call void @js_shadow_frame_pop(i64 %r6)
  ret double %common.ret.op

truthy.num.14:                                    ; preds = %if.merge.11
  %r91 = fcmp ueq double %r88, 0.000000e+00
  br i1 %r91, label %if.merge.28, label %test.right.19

truthy.tag.15:                                    ; preds = %if.merge.11
  %r93 = add i64 %r87, -9222246136947933185
  %r94 = icmp ult i64 %r93, 4
  br i1 %r94, label %truthy.merge.17, label %truthy.obj.18

truthy.slow.16:                                   ; preds = %truthy.obj.18
  %r100 = call i32 @js_is_truthy(double %r88)
  %r101.not = icmp eq i32 %r100, 0
  br i1 %r101.not, label %if.merge.28, label %test.right.19

truthy.merge.17:                                  ; preds = %truthy.tag.15
  %r92 = icmp eq i64 %r87, 9222246136947933188
  br i1 %r92, label %test.right.19, label %if.merge.28

truthy.obj.18:                                    ; preds = %truthy.tag.15
  %r95.mask = and i64 %r87, -281474976710656
  %r96 = icmp eq i64 %r95.mask, 9222527611924643840
  %r97 = and i64 %r87, 281474976710655
  %r98 = icmp ne i64 %r97, 0
  %r99 = and i1 %r96, %r98
  br i1 %r99, label %test.right.19, label %truthy.slow.16

test.right.19:                                    ; preds = %truthy.slow.16, %truthy.num.14, %truthy.obj.18, %truthy.merge.17
  %r103 = load double, ptr %r9, align 8
  %r104 = bitcast double %r103 to i64
  %r105 = and i64 %r104, 9221120237041090560
  %r106.not = icmp eq i64 %r105, 9221120237041090560
  br i1 %r106.not, label %truthy.tag.22, label %truthy.num.21

truthy.num.21:                                    ; preds = %test.right.19
  %r107 = fcmp ueq double %r103, 0.000000e+00
  br i1 %r107, label %if.then.26, label %if.merge.28

truthy.tag.22:                                    ; preds = %test.right.19
  %r109 = add i64 %r104, -9222246136947933185
  %r110 = icmp ult i64 %r109, 4
  br i1 %r110, label %truthy.merge.24, label %truthy.obj.25

truthy.slow.23:                                   ; preds = %truthy.obj.25
  %r116 = call i32 @js_is_truthy(double %r103)
  %r117.not = icmp eq i32 %r116, 0
  br i1 %r117.not, label %if.then.26, label %if.merge.28

truthy.merge.24:                                  ; preds = %truthy.tag.22
  %r108 = icmp eq i64 %r104, 9222246136947933188
  br i1 %r108, label %if.merge.28, label %if.then.26

truthy.obj.25:                                    ; preds = %truthy.tag.22
  %r111.mask = and i64 %r104, -281474976710656
  %r112 = icmp eq i64 %r111.mask, 9222527611924643840
  %r113 = and i64 %r104, 281474976710655
  %r114 = icmp ne i64 %r113, 0
  %r115 = and i1 %r112, %r114
  br i1 %r115, label %if.merge.28, label %truthy.slow.23

if.then.26:                                       ; preds = %truthy.slow.23, %truthy.num.21, %truthy.merge.24
  %r.i58 = call ptr @js_array_alloc_literal(i32 0)
  %5 = ptrtoint ptr %r.i58 to i32
  %c1.i59 = zext i32 %5 to i64
  %r131 = getelementptr i8, ptr %r.i58, i32 16
  store i32 0, ptr %r131, align 4
  %r.i61 = call i32 @js_array_mark_numeric_f64_layout(ptr %r.i58)
  %r133 = or disjoint i64 %c1.i59, 9222527611924643840
  store i64 %r133, ptr %r128, align 8
  call void @js_shadow_slot_bind(i32 3, ptr nonnull %r128)
  %r135 = load double, ptr %r8, align 8
  store volatile double %r135, ptr %r121, align 8
  %r138 = load volatile double, ptr %r121, align 8
  store volatile double %r138, ptr %r121, align 8
  %r139 = call i32 @js_array_record_stack_enter(ptr nonnull %r121, i32 3, i32 0)
  %r140 = icmp ne i32 %r139, 0
  %r147 = uitofp i1 %r140 to double
  store double %r147, ptr %r126, align 8
  %r148 = load volatile double, ptr %r121, align 8
  store double 0x7FFC000000000001, ptr %r149, align 8
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r151)
  %r235 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.311.dispatch to i64), 9221120237041090560
  %r260 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.124.dispatch to i64), 9221120237041090560
  %r284 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.251.dispatch to i64), 9221120237041090560
  br label %for.cond.36

if.merge.28:                                      ; preds = %truthy.slow.23, %truthy.num.21, %truthy.slow.16, %truthy.num.14, %truthy.obj.25, %plen.merge.104, %truthy.merge.24, %truthy.merge.17
  %r473 = load double, ptr %r8, align 8
  store volatile double %r473, ptr %r21, align 8
  %r476 = load volatile double, ptr %r21, align 8
  store volatile double %r476, ptr %r21, align 8
  %r477 = call i32 @js_array_record_stack_enter(ptr nonnull %r21, i32 3, i32 0)
  %r478 = icmp ne i32 %r477, 0
  %r485 = uitofp i1 %r478 to double
  store double %r485, ptr %r26, align 8
  %r486 = load volatile double, ptr %r21, align 8
  store double 0x7FFC000000000001, ptr %r487, align 8
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r489)
  %r677 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.140.dispatch to i64), 9221120237041090560
  %r684 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.317.dispatch to i64), 9221120237041090560
  br label %for.cond.118

try.catch.30:                                     ; preds = %wasm.eh.accepted.34
  %r321 = call double @js_catch_enter()
  store double %r321, ptr %r322, align 8
  call void @js_shadow_slot_bind(i32 9, ptr nonnull %r322)
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r323)
  br i1 %r150.0, label %if.then.80, label %if.merge.82

wasm.eh.dispatch.32:                              ; preds = %alen.slow.67, %apush.spec.done.66, %eh.cont288, %idispatch.merge.61, %record.next.guard.40, %eh.cont279, %logical.then.58, %str_addref.done.50, %gcpoll.69, %truthy.slow.55, %str_addref.call.49, %gcpoll.47, %record.next.cold.45
  %r150.0 = phi i1 [ true, %gcpoll.69 ], [ true, %alen.slow.67 ], [ true, %apush.spec.done.66 ], [ true, %eh.cont288 ], [ true, %eh.cont279 ], [ true, %idispatch.merge.61 ], [ true, %logical.then.58 ], [ true, %truthy.slow.55 ], [ true, %str_addref.done.50 ], [ true, %str_addref.call.49 ], [ false, %gcpoll.47 ], [ false, %record.next.cold.45 ], [ false, %record.next.guard.40 ]
  %r152 = catchswitch within none [label %wasm.eh.pad.33] unwind to caller

wasm.eh.pad.33:                                   ; preds = %wasm.eh.dispatch.32
  %r153 = catchpad within %r152 [ptr null]
  %r154 = call ptr @llvm.wasm.get.exception(token %r153)
  %r155 = call i1 @js_wasm_eh_is_perry(ptr %r154) [ "funclet"(token %r153) ]
  br i1 %r155, label %wasm.eh.accepted.34, label %wasm.eh.foreign.35

wasm.eh.accepted.34:                              ; preds = %wasm.eh.pad.33
  call void @js_wasm_eh_pad_enter(ptr nonnull %r151) [ "funclet"(token %r153) ]
  catchret from %r153 to label %try.catch.30

wasm.eh.foreign.35:                               ; preds = %wasm.eh.pad.33
  call void @llvm.wasm.rethrow() [ "funclet"(token %r153) ]
  unreachable

for.cond.36:                                      ; preds = %gcpoll.done.70, %if.then.26
  %r143.0 = phi double [ 0.000000e+00, %if.then.26 ], [ %r320, %gcpoll.done.70 ]
  %r160 = load volatile double, ptr %r121, align 8
  br i1 %r140, label %record.next.cold.45, label %record.next.guard.40

for.body.37:                                      ; preds = %gcpoll.done.48
  %r226 = load double, ptr %r149, align 8
  %r227 = bitcast double %r226 to i64
  %r228 = and i64 %r227, -281474976710656
  %r229.not = icmp eq i64 %r228, 9223090561878065152
  br i1 %r229.not, label %str_addref.call.49, label %str_addref.done.50

record.next.guard.40:                             ; preds = %for.cond.36
  %r170 = bitcast double %r160 to i64
  %6 = trunc i64 %r170 to i32
  %7 = add i32 %6, -8
  %r173 = inttoptr i32 %7 to ptr
  %r174 = load i32, ptr %r173, align 4
  %c1.i = inttoptr i32 %6 to ptr
  %r.i4243 = invoke i32 @perry_array_element_kind(ptr %c1.i)
          to label %eh.cont178 unwind label %wasm.eh.dispatch.32

eh.cont178:                                       ; preds = %record.next.guard.40
  %r175 = and i32 %r174, 33023
  %r176 = icmp eq i32 %r175, 1
  %r179.not = icmp eq i32 %r.i4243, 0
  %r181 = and i1 %r176, %r179.not
  %r182 = load i8, ptr @PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED, align 1
  %r183 = and i8 %r182, 1
  %r184 = icmp eq i8 %r183, 0
  %r185 = and i1 %r181, %r184
  br i1 %r185, label %record.next.bounds.41, label %record.next.cold.45

record.next.bounds.41:                            ; preds = %eh.cont178
  %r186 = fptoui double %r143.0 to i32
  %r1417 = load volatile double, ptr %r121, align 8
  %r1413 = load volatile double, ptr %r121, align 8
  %r1414 = bitcast double %r1413 to i64
  %8 = trunc i64 %r1414 to i32
  %r1416 = inttoptr i32 %8 to ptr
  %r188 = load i32, ptr %r1416, align 4
  %r189 = icmp ugt i32 %r188, %r186
  %r1409 = load volatile double, ptr %r121, align 8
  %r1410 = bitcast double %r1409 to i64
  %9 = trunc i64 %r1410 to i32
  %r1412 = inttoptr i32 %9 to ptr
  %r190 = getelementptr i8, ptr %r1412, i32 4
  %r191 = load i32, ptr %r190, align 4
  %r192 = icmp ugt i32 %r191, %r186
  %r193 = and i1 %r189, %r192
  br i1 %r193, label %record.next.load.43, label %record.next.unloadable.42

record.next.unloadable.42:                        ; preds = %record.next.bounds.41
  br i1 %r189, label %record.next.cold.45, label %record.next.merge.46

record.next.load.43:                              ; preds = %record.next.bounds.41
  %r194 = zext i32 %r186 to i64
  %r195 = shl nuw nsw i64 %r194, 3
  %r1406 = load volatile double, ptr %r121, align 8
  %r1402 = load volatile double, ptr %r121, align 8
  %r1397 = load volatile double, ptr %r121, align 8
  %r1398 = bitcast double %r1397 to i64
  %10 = trunc i64 %r1398 to i32
  %11 = add i32 %10, 4
  %r1401 = inttoptr i32 %11 to ptr
  %r198 = load i32, ptr %r1401, align 4
  %r1394 = load volatile double, ptr %r121, align 8
  %r1395 = bitcast double %r1394 to i64
  %12 = trunc i64 %r1395 to i32
  %13 = add i32 %12, -4
  %r200 = inttoptr i32 %13 to ptr
  %r201 = load i32, ptr %r200, align 4
  %r202 = zext i32 %r201 to i64
  %r203 = zext i32 %r198 to i64
  %r1391 = load volatile double, ptr %r121, align 8
  %r1392 = bitcast double %r1391 to i64
  %r205 = add nuw nsw i64 %r195, 4294967288
  %14 = shl nuw nsw i64 %r203, 3
  %r206 = sub nsw i64 %r205, %14
  %r207 = add nsw i64 %r206, %r202
  %r208 = add i64 %r207, %r1392
  %15 = trunc i64 %r208 to i32
  %r209 = inttoptr i32 %15 to ptr
  %r210 = load double, ptr %r209, align 8
  %r211 = bitcast double %r210 to i64
  %r212 = icmp eq i64 %r211, 9222246136947933200
  %r214 = select i1 %r212, double 0x7FFC000000000001, double %r210
  br label %record.next.merge.46

record.next.cold.45:                              ; preds = %record.next.unloadable.42, %eh.cont178, %for.cond.36
  store double %r143.0, ptr %r124, align 8
  %r215 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r121, i32 6)
          to label %eh.cont216 unwind label %wasm.eh.dispatch.32

eh.cont216:                                       ; preds = %record.next.cold.45
  %r217 = bitcast double %r215 to i64
  %r218 = icmp eq i64 %r217, 9222246136947933187
  %r219 = load volatile double, ptr %r127, align 8
  store volatile double 0x7FFC000000000001, ptr %r127, align 8
  br label %record.next.merge.46

record.next.merge.46:                             ; preds = %record.next.unloadable.42, %eh.cont216, %record.next.load.43
  %r220 = phi double [ %r214, %record.next.load.43 ], [ %r219, %eh.cont216 ], [ 0x7FFC000000000001, %record.next.unloadable.42 ]
  %r221 = phi i1 [ true, %record.next.load.43 ], [ %r218, %eh.cont216 ], [ false, %record.next.unloadable.42 ]
  store double %r220, ptr %r149, align 8
  call void @js_shadow_slot_bind(i32 7, ptr nonnull %r149)
  %r222 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r223.not = icmp eq i32 %r222, 0
  br i1 %r223.not, label %gcpoll.done.48, label %gcpoll.47

gcpoll.47:                                        ; preds = %record.next.merge.46
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.48 unwind label %wasm.eh.dispatch.32

gcpoll.done.48:                                   ; preds = %gcpoll.47, %record.next.merge.46
  br i1 %r221, label %for.body.37, label %if.then.98

str_addref.call.49:                               ; preds = %for.body.37
  invoke void @js_string_addref_if_heap_string(double %r226)
          to label %str_addref.call.49.str_addref.done.50_crit_edge unwind label %wasm.eh.dispatch.32

str_addref.call.49.str_addref.done.50_crit_edge:  ; preds = %str_addref.call.49
  %r1390.pre = load double, ptr %r149, align 8
  br label %str_addref.done.50

str_addref.done.50:                               ; preds = %str_addref.call.49.str_addref.done.50_crit_edge, %for.body.37
  %r1390 = phi double [ %r1390.pre, %str_addref.call.49.str_addref.done.50_crit_edge ], [ %r226, %for.body.37 ]
  store double %r1390, ptr %r225, align 8
  call void @js_shadow_slot_bind(i32 8, ptr nonnull %r225)
  %r231 = load double, ptr %r225, align 8
  %r.i2122 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347282, double %r231, i64 range(i64 9221120237041090560, 0) %r235, ptr null, i32 0)
          to label %idispatch.merge.52 unwind label %wasm.eh.dispatch.32

idispatch.merge.52:                               ; preds = %str_addref.done.50
  %r239 = bitcast double %r.i2122 to i64
  %r240 = and i64 %r239, 9221120237041090560
  %r241.not = icmp eq i64 %r240, 9221120237041090560
  br i1 %r241.not, label %truthy.tag.54, label %truthy.num.53

truthy.num.53:                                    ; preds = %idispatch.merge.52
  %r242 = fcmp ueq double %r.i2122, 0.000000e+00
  br i1 %r242, label %logical.merge.59, label %logical.then.58

truthy.tag.54:                                    ; preds = %idispatch.merge.52
  %r244 = add i64 %r239, -9222246136947933185
  %r245 = icmp ult i64 %r244, 4
  br i1 %r245, label %truthy.merge.56, label %truthy.obj.57

truthy.slow.55:                                   ; preds = %truthy.obj.57
  %r251 = invoke i32 @js_is_truthy(double %r.i2122)
          to label %eh.cont252 unwind label %wasm.eh.dispatch.32

eh.cont252:                                       ; preds = %truthy.slow.55
  %r253.not = icmp eq i32 %r251, 0
  br i1 %r253.not, label %logical.merge.59, label %logical.then.58

truthy.merge.56:                                  ; preds = %truthy.tag.54
  %r243 = icmp eq i64 %r239, 9222246136947933188
  br i1 %r243, label %logical.then.58, label %logical.merge.59

truthy.obj.57:                                    ; preds = %truthy.tag.54
  %r246.mask = and i64 %r239, -281474976710656
  %r247 = icmp eq i64 %r246.mask, 9222527611924643840
  %r248 = and i64 %r239, 281474976710655
  %r249 = icmp ne i64 %r248, 0
  %r250 = and i1 %r247, %r249
  br i1 %r250, label %logical.then.58, label %truthy.slow.55

logical.then.58:                                  ; preds = %eh.cont252, %truthy.num.53, %truthy.obj.57, %truthy.merge.56
  %r255 = load double, ptr %r128, align 8
  store double %r255, ptr %r67, align 8
  call void @js_shadow_slot_bind(i32 37, ptr nonnull %r67)
  %r256 = load double, ptr %r225, align 8
  %r.i2324 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347283, double %r256, i64 range(i64 9221120237041090560, 0) %r260, ptr null, i32 0)
          to label %idispatch.merge.61 unwind label %wasm.eh.dispatch.32

logical.merge.59:                                 ; preds = %eh.cont252, %truthy.num.53, %apush.own.join.62, %truthy.merge.56
  call void @js_shadow_slot_set(i32 8, i64 0)
  %r316 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r317.not = icmp eq i32 %r316, 0
  br i1 %r317.not, label %gcpoll.done.70, label %gcpoll.69

idispatch.merge.61:                               ; preds = %logical.then.58
  store double %r.i2324, ptr %r73, align 8
  call void @js_shadow_slot_bind(i32 38, ptr nonnull %r73)
  %r.i5557 = invoke ptr @js_closure_alloc_empty(ptr nonnull @"perry_closure_node_modules_glob_dist_commonjs_index_min_js__621$info", i32 -1543503872)
          to label %eh.cont279 unwind label %wasm.eh.dispatch.32

eh.cont279:                                       ; preds = %idispatch.merge.61
  %16 = ptrtoint ptr %r.i5557 to i32
  %c1.i56 = zext i32 %16 to i64
  %r280 = or disjoint i64 %c1.i56, 9222527611924643840
  %r282 = load double, ptr %r73, align 8
  store i64 %r280, ptr %r285, align 8
  %r.i2627 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347284, double %r282, i64 range(i64 9221120237041090560, 0) %r284, ptr nonnull %r285, i32 1)
          to label %eh.cont288 unwind label %wasm.eh.dispatch.32

eh.cont288:                                       ; preds = %eh.cont279
  store double 0.000000e+00, ptr %r73, align 8
  call void @js_shadow_slot_set(i32 38, i64 0)
  %r29018 = load i64, ptr %r67, align 8
  %r29119 = load i64, ptr %r128, align 8
  %r294 = icmp eq i64 %r29119, %r29018
  %17 = trunc i64 %r29018 to i32
  %c1.i62 = inttoptr i32 %17 to ptr
  %r.i6364 = invoke i64 @js_array_push_f64_spec_or_own(ptr %c1.i62, double %r.i2627, ptr nonnull %r297)
          to label %eh.cont299 unwind label %wasm.eh.dispatch.32

eh.cont299:                                       ; preds = %eh.cont288
  %r300 = load i32, ptr %r297, align 4
  %r301.not = icmp eq i32 %r300, 0
  br i1 %r301.not, label %apush.own.builtin.64, label %apush.own.join.62, !prof !0

apush.own.join.62:                                ; preds = %alen.slow.67, %eh.cont307, %eh.cont299
  store double 0.000000e+00, ptr %r67, align 8
  call void @js_shadow_slot_set(i32 37, i64 0)
  br label %logical.merge.59

apush.own.builtin.64:                             ; preds = %eh.cont299
  br i1 %r294, label %apush.spec.writeback.65, label %apush.spec.done.66

apush.spec.writeback.65:                          ; preds = %apush.own.builtin.64
  %r304 = or i64 %r.i6364, 9222527611924643840
  store i64 %r304, ptr %r128, align 8
  br label %apush.spec.done.66

apush.spec.done.66:                               ; preds = %apush.spec.writeback.65, %apush.own.builtin.64
  %18 = trunc i64 %r.i6364 to i32
  %c1.i85 = inttoptr i32 %18 to ptr
  %r.i8687 = invoke i64 @js_array_length_leaf(ptr %c1.i85)
          to label %eh.cont307 unwind label %wasm.eh.dispatch.32

eh.cont307:                                       ; preds = %apush.spec.done.66
  %r308 = icmp slt i64 %r.i8687, 0
  br i1 %r308, label %alen.slow.67, label %apush.own.join.62

alen.slow.67:                                     ; preds = %eh.cont307
  %r.i9394 = invoke i32 @js_array_length(ptr %c1.i85)
          to label %apush.own.join.62 unwind label %wasm.eh.dispatch.32

gcpoll.69:                                        ; preds = %logical.merge.59
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.70 unwind label %wasm.eh.dispatch.32

gcpoll.done.70:                                   ; preds = %gcpoll.69, %logical.merge.59
  %r320 = fadd double %r143.0, 1.000000e+00
  br label %for.cond.36

try.catch.fail.72:                                ; preds = %wasm.eh.accepted.75
  %r355 = call double @js_get_exception()
  store double %r355, ptr %r356, align 8
  call void @js_shadow_slot_bind(i32 45, ptr nonnull %r356)
  store volatile double 0x7FFC000000000001, ptr %r121, align 8
  store double 0x7FFC000000000001, ptr %r149, align 8
  call void @js_shadow_slot_set(i32 7, i64 0)
  %r385 = load double, ptr %r356, align 8
  store double 0.000000e+00, ptr %r356, align 8
  call void @js_throw(double %r385)
  unreachable

wasm.eh.dispatch.73:                              ; preds = %if.merge.82, %eh.cont351, %if.then.80
  %r324 = catchswitch within none [label %wasm.eh.pad.74] unwind to caller

wasm.eh.pad.74:                                   ; preds = %wasm.eh.dispatch.73
  %r325 = catchpad within %r324 [ptr null]
  %r326 = call ptr @llvm.wasm.get.exception(token %r325)
  %r327 = call i1 @js_wasm_eh_is_perry(ptr %r326) [ "funclet"(token %r325) ]
  br i1 %r327, label %wasm.eh.accepted.75, label %wasm.eh.foreign.76

wasm.eh.accepted.75:                              ; preds = %wasm.eh.pad.74
  call void @js_wasm_eh_pad_enter(ptr nonnull %r323) [ "funclet"(token %r325) ]
  catchret from %r325 to label %try.catch.fail.72

wasm.eh.foreign.76:                               ; preds = %wasm.eh.pad.74
  call void @llvm.wasm.rethrow() [ "funclet"(token %r325) ]
  unreachable

if.then.80:                                       ; preds = %try.catch.30
  %r344 = fadd double %r143.0, 1.000000e+00
  store double %r344, ptr %r124, align 8
  store double 0.000000e+00, ptr %r125, align 8
  %r349 = load double, ptr %r322, align 8
  store volatile double %r349, ptr %r127, align 8
  %r350 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r121, i32 4)
          to label %eh.cont351 unwind label %wasm.eh.dispatch.73

eh.cont351:                                       ; preds = %if.then.80
  invoke void @js_throw(double %r350)
          to label %eh.cont352 unwind label %wasm.eh.dispatch.73

eh.cont352:                                       ; preds = %eh.cont351
  unreachable

if.merge.82:                                      ; preds = %try.catch.30
  %r353 = load double, ptr %r322, align 8
  invoke void @js_throw(double %r353)
          to label %eh.cont354 unwind label %wasm.eh.dispatch.73

eh.cont354:                                       ; preds = %if.merge.82
  unreachable

if.then.98:                                       ; preds = %gcpoll.done.48
  store volatile double 0x7FFC000000000001, ptr %r121, align 8
  store double 0x7FFC000000000001, ptr %r149, align 8
  call void @js_shadow_slot_set(i32 7, i64 0)
  %r414 = load double, ptr %r128, align 8
  %r415 = bitcast double %r414 to i64
  %r417 = add i64 %r415, -9222527611926740992
  %r418 = icmp ult i64 %r417, 281474974613504
  br i1 %r418, label %plen.check_gc.101, label %plen.slow.103

plen.check_gc.101:                                ; preds = %if.then.98
  %19 = trunc i64 %r415 to i32
  %20 = add i32 %19, -8
  %r420 = inttoptr i32 %20 to ptr
  %r421 = load i16, ptr %r420, align 2
  %r422 = and i16 %r421, -32513
  %r423 = icmp eq i16 %r422, 1
  br i1 %r423, label %plen.fast.102, label %plen.slow.103

plen.fast.102:                                    ; preds = %plen.check_gc.101
  %r424 = inttoptr i32 %19 to ptr
  %r425 = load i32, ptr %r424, align 4
  %r426 = uitofp i32 %r425 to double
  br label %plen.merge.104

plen.slow.103:                                    ; preds = %plen.check_gc.101, %if.then.98
  %r427 = call double @js_value_length_property_f64(double %r414)
  br label %plen.merge.104

plen.merge.104:                                   ; preds = %plen.slow.103, %plen.fast.102
  %r428 = phi double [ %r426, %plen.fast.102 ], [ %r427, %plen.slow.103 ]
  %r429 = fcmp ueq double %r428, 0.000000e+00
  br i1 %r429, label %if.merge.28, label %if.then.105

if.then.105:                                      ; preds = %plen.merge.104
  %r430 = call ptr @perry_intrinsic_proto(i32 24)
  %r431 = getelementptr i8, ptr %r430, i32 4
  %r432 = load i32, ptr %r431, align 4
  %r433 = icmp ugt i32 %r432, -1879048193
  br i1 %r433, label %ns_guard.generic.108, label %ns_guard.intrinsic.109, !prof !1

ns_guard.generic.108:                             ; preds = %if.then.105
  %r435 = call double @js_builtin_namespace_value(i64 24)
  %r.i82 = call double @js_builtin_namespace_member(double %r435, ptr nonnull @node_modules_glob_dist_commonjs_index_min_js_.str.316.bytes, i32 3)
  %r437 = load double, ptr %r128, align 8
  store double %r437, ptr %r438, align 8
  %r.i83 = call double @js_method_call_value(double %r.i82, double %r435, ptr nonnull %r438, i32 1)
  br label %ns_guard.merge.110

ns_guard.intrinsic.109:                           ; preds = %if.then.105
  %r441 = load double, ptr %r128, align 8
  %r.i99 = call ptr @js_promise_all_iterable(double %r441)
  %21 = ptrtoint ptr %r.i99 to i32
  %c1.i100 = zext i32 %21 to i64
  %r443 = or disjoint i64 %c1.i100, 9222527611924643840
  %r444 = bitcast i64 %r443 to double
  br label %ns_guard.merge.110

ns_guard.merge.110:                               ; preds = %ns_guard.intrinsic.109, %ns_guard.generic.108
  %r445 = phi double [ %r.i83, %ns_guard.generic.108 ], [ %r444, %ns_guard.intrinsic.109 ]
  store double %r445, ptr %r67, align 8
  call void @js_shadow_slot_bind(i32 37, ptr nonnull %r67)
  %r446 = load i64, ptr %r10, align 8
  %22 = trunc i64 %r446 to i32
  %c1.i78 = inttoptr i32 %22 to ptr
  %r.i79 = call i64 @js_closure_get_capture_bits(ptr %c1.i78, i32 8)
  %r44817 = load i64, ptr %r8, align 8
  %r1388 = load i64, ptr %r10, align 8
  %23 = trunc i64 %r1388 to i32
  %c1.i80 = inttoptr i32 %23 to ptr
  %r.i81 = call i64 @js_closure_get_capture_bits(ptr %c1.i80, i32 8)
  %r.i53 = call ptr @js_closure_alloc_small(ptr nonnull @"perry_closure_node_modules_glob_dist_commonjs_index_min_js__622$info", i64 -6629298651489370110, i64 %r.i81, i64 %r44817, i64 0)
  %24 = ptrtoint ptr %r.i53 to i32
  %c1.i54 = zext i32 %24 to i64
  %r465 = or disjoint i64 %c1.i54, 9222527611924643840
  %r467 = load double, ptr %r67, align 8
  store i64 %r465, ptr %r470, align 8
  %r.i29 = call double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347285, double %r467, i64 range(i64 9221120237041090560, 0) %r284, ptr nonnull %r470, i32 1)
  store double 0.000000e+00, ptr %r67, align 8
  call void @js_shadow_slot_set(i32 37, i64 0)
  br label %common.ret

try.catch.112:                                    ; preds = %wasm.eh.accepted.116
  %r742 = call double @js_catch_enter()
  store double %r742, ptr %r743, align 8
  call void @js_shadow_slot_bind(i32 15, ptr nonnull %r743)
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r744)
  br i1 %r488.0, label %if.then.189, label %if.merge.191

wasm.eh.dispatch.114:                             ; preds = %callee_unbox.merge.148, %record.next.guard.122, %ternary.merge.163, %ternary.else.162, %gcpoll.178, %truthy.slow.170, %truthy.slow.158, %truthy.slow.151, %callee_unbox.slow.147, %truthy.slow.142, %truthy.slow.135, %str_addref.call.131, %gcpoll.129, %record.next.cold.127
  %r488.0 = phi i1 [ true, %gcpoll.178 ], [ false, %record.next.guard.122 ], [ true, %truthy.slow.170 ], [ true, %ternary.merge.163 ], [ true, %ternary.else.162 ], [ true, %truthy.slow.158 ], [ true, %truthy.slow.151 ], [ true, %callee_unbox.merge.148 ], [ true, %callee_unbox.slow.147 ], [ true, %truthy.slow.142 ], [ true, %truthy.slow.135 ], [ true, %str_addref.call.131 ], [ false, %gcpoll.129 ], [ false, %record.next.cold.127 ]
  %r490 = catchswitch within none [label %wasm.eh.pad.115] unwind to caller

wasm.eh.pad.115:                                  ; preds = %wasm.eh.dispatch.114
  %r491 = catchpad within %r490 [ptr null]
  %r492 = call ptr @llvm.wasm.get.exception(token %r491)
  %r493 = call i1 @js_wasm_eh_is_perry(ptr %r492) [ "funclet"(token %r491) ]
  br i1 %r493, label %wasm.eh.accepted.116, label %wasm.eh.foreign.117

wasm.eh.accepted.116:                             ; preds = %wasm.eh.pad.115
  call void @js_wasm_eh_pad_enter(ptr nonnull %r489) [ "funclet"(token %r491) ]
  catchret from %r491 to label %try.catch.112

wasm.eh.foreign.117:                              ; preds = %wasm.eh.pad.115
  call void @llvm.wasm.rethrow() [ "funclet"(token %r491) ]
  unreachable

for.cond.118:                                     ; preds = %gcpoll.done.179, %if.merge.28
  %r481.0 = phi double [ 0.000000e+00, %if.merge.28 ], [ %r741, %gcpoll.done.179 ]
  %r498 = load volatile double, ptr %r21, align 8
  br i1 %r478, label %record.next.cold.127, label %record.next.guard.122

for.body.119:                                     ; preds = %gcpoll.done.130
  %r564 = load double, ptr %r487, align 8
  %r565 = bitcast double %r564 to i64
  %r566 = and i64 %r565, -281474976710656
  %r567.not = icmp eq i64 %r566, 9223090561878065152
  br i1 %r567.not, label %str_addref.call.131, label %str_addref.done.132

record.next.guard.122:                            ; preds = %for.cond.118
  %r508 = bitcast double %r498 to i64
  %25 = trunc i64 %r508 to i32
  %26 = add i32 %25, -8
  %r511 = inttoptr i32 %26 to ptr
  %r512 = load i32, ptr %r511, align 4
  %c1.i44 = inttoptr i32 %25 to ptr
  %r.i4546 = invoke i32 @perry_array_element_kind(ptr %c1.i44)
          to label %eh.cont516 unwind label %wasm.eh.dispatch.114

eh.cont516:                                       ; preds = %record.next.guard.122
  %r513 = and i32 %r512, 33023
  %r514 = icmp eq i32 %r513, 1
  %r517.not = icmp eq i32 %r.i4546, 0
  %r519 = and i1 %r514, %r517.not
  %r520 = load i8, ptr @PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED, align 1
  %r521 = and i8 %r520, 1
  %r522 = icmp eq i8 %r521, 0
  %r523 = and i1 %r519, %r522
  br i1 %r523, label %record.next.bounds.123, label %record.next.cold.127

record.next.bounds.123:                           ; preds = %eh.cont516
  %r524 = fptoui double %r481.0 to i32
  %r1385 = load volatile double, ptr %r21, align 8
  %r1381 = load volatile double, ptr %r21, align 8
  %r1382 = bitcast double %r1381 to i64
  %27 = trunc i64 %r1382 to i32
  %r1384 = inttoptr i32 %27 to ptr
  %r526 = load i32, ptr %r1384, align 4
  %r527 = icmp ugt i32 %r526, %r524
  %r1377 = load volatile double, ptr %r21, align 8
  %r1378 = bitcast double %r1377 to i64
  %28 = trunc i64 %r1378 to i32
  %r1380 = inttoptr i32 %28 to ptr
  %r528 = getelementptr i8, ptr %r1380, i32 4
  %r529 = load i32, ptr %r528, align 4
  %r530 = icmp ugt i32 %r529, %r524
  %r531 = and i1 %r527, %r530
  br i1 %r531, label %record.next.load.125, label %record.next.unloadable.124

record.next.unloadable.124:                       ; preds = %record.next.bounds.123
  br i1 %r527, label %record.next.cold.127, label %record.next.merge.128

record.next.load.125:                             ; preds = %record.next.bounds.123
  %r532 = zext i32 %r524 to i64
  %r533 = shl nuw nsw i64 %r532, 3
  %r1374 = load volatile double, ptr %r21, align 8
  %r1370 = load volatile double, ptr %r21, align 8
  %r1365 = load volatile double, ptr %r21, align 8
  %r1366 = bitcast double %r1365 to i64
  %29 = trunc i64 %r1366 to i32
  %30 = add i32 %29, 4
  %r1369 = inttoptr i32 %30 to ptr
  %r536 = load i32, ptr %r1369, align 4
  %r1362 = load volatile double, ptr %r21, align 8
  %r1363 = bitcast double %r1362 to i64
  %31 = trunc i64 %r1363 to i32
  %32 = add i32 %31, -4
  %r538 = inttoptr i32 %32 to ptr
  %r539 = load i32, ptr %r538, align 4
  %r540 = zext i32 %r539 to i64
  %r541 = zext i32 %r536 to i64
  %r1359 = load volatile double, ptr %r21, align 8
  %r1360 = bitcast double %r1359 to i64
  %r543 = add nuw nsw i64 %r533, 4294967288
  %33 = shl nuw nsw i64 %r541, 3
  %r544 = sub nsw i64 %r543, %33
  %r545 = add nsw i64 %r544, %r540
  %r546 = add i64 %r545, %r1360
  %34 = trunc i64 %r546 to i32
  %r547 = inttoptr i32 %34 to ptr
  %r548 = load double, ptr %r547, align 8
  %r549 = bitcast double %r548 to i64
  %r550 = icmp eq i64 %r549, 9222246136947933200
  %r552 = select i1 %r550, double 0x7FFC000000000001, double %r548
  br label %record.next.merge.128

record.next.cold.127:                             ; preds = %record.next.unloadable.124, %eh.cont516, %for.cond.118
  store double %r481.0, ptr %r24, align 8
  %r553 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r21, i32 6)
          to label %eh.cont554 unwind label %wasm.eh.dispatch.114

eh.cont554:                                       ; preds = %record.next.cold.127
  %r555 = bitcast double %r553 to i64
  %r556 = icmp eq i64 %r555, 9222246136947933187
  %r557 = load volatile double, ptr %r27, align 8
  store volatile double 0x7FFC000000000001, ptr %r27, align 8
  br label %record.next.merge.128

record.next.merge.128:                            ; preds = %record.next.unloadable.124, %eh.cont554, %record.next.load.125
  %r558 = phi double [ %r552, %record.next.load.125 ], [ %r557, %eh.cont554 ], [ 0x7FFC000000000001, %record.next.unloadable.124 ]
  %r559 = phi i1 [ true, %record.next.load.125 ], [ %r556, %eh.cont554 ], [ false, %record.next.unloadable.124 ]
  store double %r558, ptr %r487, align 8
  call void @js_shadow_slot_bind(i32 13, ptr nonnull %r487)
  %r560 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r561.not = icmp eq i32 %r560, 0
  br i1 %r561.not, label %gcpoll.done.130, label %gcpoll.129

gcpoll.129:                                       ; preds = %record.next.merge.128
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.130 unwind label %wasm.eh.dispatch.114

gcpoll.done.130:                                  ; preds = %gcpoll.129, %record.next.merge.128
  br i1 %r559, label %for.body.119, label %if.then.207

str_addref.call.131:                              ; preds = %for.body.119
  invoke void @js_string_addref_if_heap_string(double %r564)
          to label %str_addref.call.131.str_addref.done.132_crit_edge unwind label %wasm.eh.dispatch.114

str_addref.call.131.str_addref.done.132_crit_edge: ; preds = %str_addref.call.131
  %r1358.pre = load double, ptr %r487, align 8
  br label %str_addref.done.132

str_addref.done.132:                              ; preds = %str_addref.call.131.str_addref.done.132_crit_edge, %for.body.119
  %r1358 = phi double [ %r1358.pre, %str_addref.call.131.str_addref.done.132_crit_edge ], [ %r564, %for.body.119 ]
  store double %r1358, ptr %r563, align 8
  call void @js_shadow_slot_bind(i32 14, ptr nonnull %r563)
  %r569 = load double, ptr %r563, align 8
  %r570 = bitcast double %r569 to i64
  %r571 = and i64 %r570, 9221120237041090560
  %r572.not = icmp eq i64 %r571, 9221120237041090560
  br i1 %r572.not, label %truthy.tag.134, label %truthy.num.133

truthy.num.133:                                   ; preds = %str_addref.done.132
  %r573 = fcmp ueq double %r569, 0.000000e+00
  br i1 %r573, label %logical.merge.155, label %logical.then.138

truthy.tag.134:                                   ; preds = %str_addref.done.132
  %r575 = add i64 %r570, -9222246136947933185
  %r576 = icmp ult i64 %r575, 4
  br i1 %r576, label %truthy.merge.136, label %truthy.obj.137

truthy.slow.135:                                  ; preds = %truthy.obj.137
  %r582 = invoke i32 @js_is_truthy(double %r569)
          to label %eh.cont583 unwind label %wasm.eh.dispatch.114

eh.cont583:                                       ; preds = %truthy.slow.135
  %r584.not = icmp eq i32 %r582, 0
  br i1 %r584.not, label %logical.merge.155, label %logical.then.138

truthy.merge.136:                                 ; preds = %truthy.tag.134
  %r574 = icmp eq i64 %r570, 9222246136947933188
  br i1 %r574, label %logical.then.138, label %logical.merge.155

truthy.obj.137:                                   ; preds = %truthy.tag.134
  %r577.mask = and i64 %r570, -281474976710656
  %r578 = icmp eq i64 %r577.mask, 9222527611924643840
  %r579 = and i64 %r570, 281474976710655
  %r580 = icmp ne i64 %r579, 0
  %r581 = and i1 %r578, %r580
  br i1 %r581, label %logical.then.138, label %truthy.slow.135

logical.then.138:                                 ; preds = %eh.cont583, %truthy.num.133, %truthy.obj.137, %truthy.merge.136
  %r586 = load i64, ptr %r10, align 8
  %35 = trunc i64 %r586 to i32
  %36 = add i32 %35, 40
  %r588 = inttoptr i32 %36 to ptr
  %r589 = load i64, ptr %r588, align 8
  %r590 = bitcast i64 %r589 to double
  %r591 = and i64 %r589, 9221120237041090560
  %r592.not = icmp eq i64 %r591, 9221120237041090560
  br i1 %r592.not, label %truthy.tag.141, label %truthy.num.140

logical.merge.139:                                ; preds = %truthy.tag.150
  %r628 = icmp eq i64 %r624, 9222246136947933188
  br i1 %r628, label %logical.then.154, label %logical.merge.155

truthy.num.140:                                   ; preds = %logical.then.138
  %r593 = fcmp ueq double %r590, 0.000000e+00
  br i1 %r593, label %logical.then.154, label %logical.then.145

truthy.tag.141:                                   ; preds = %logical.then.138
  %r595 = add i64 %r589, -9222246136947933185
  %r596 = icmp ult i64 %r595, 4
  br i1 %r596, label %truthy.merge.143, label %truthy.obj.144

truthy.slow.142:                                  ; preds = %truthy.obj.144
  %r602 = invoke i32 @js_is_truthy(double %r590)
          to label %eh.cont603 unwind label %wasm.eh.dispatch.114

eh.cont603:                                       ; preds = %truthy.slow.142
  %r604.not = icmp eq i32 %r602, 0
  br i1 %r604.not, label %logical.then.154, label %logical.then.145

truthy.merge.143:                                 ; preds = %truthy.tag.141
  %r594 = icmp eq i64 %r589, 9222246136947933188
  br i1 %r594, label %callee_unbox.slow.147, label %logical.then.154

truthy.obj.144:                                   ; preds = %truthy.tag.141
  %r597.mask = and i64 %r589, -281474976710656
  %r598 = icmp eq i64 %r597.mask, 9222527611924643840
  %r599 = and i64 %r589, 281474976710655
  %r600 = icmp ne i64 %r599, 0
  %r601 = and i1 %r598, %r600
  br i1 %r601, label %logical.then.145, label %truthy.slow.142

logical.then.145:                                 ; preds = %eh.cont603, %truthy.num.140, %truthy.obj.144
  %r616.mask = and i64 %r589, -281474976710656
  %r617 = icmp eq i64 %r616.mask, 9222527611924643840
  br i1 %r617, label %callee_unbox.merge.148, label %callee_unbox.slow.147

callee_unbox.slow.147:                            ; preds = %truthy.merge.143, %logical.then.145
  %r619 = invoke i64 @js_closure_unbox_callee_checked(double %r590)
          to label %callee_unbox.slow.147.callee_unbox.merge.148_crit_edge unwind label %wasm.eh.dispatch.114

callee_unbox.slow.147.callee_unbox.merge.148_crit_edge: ; preds = %callee_unbox.slow.147
  %r1357.pre = load double, ptr %r563, align 8
  br label %callee_unbox.merge.148

callee_unbox.merge.148:                           ; preds = %callee_unbox.slow.147.callee_unbox.merge.148_crit_edge, %logical.then.145
  %r1357 = phi double [ %r569, %logical.then.145 ], [ %r1357.pre, %callee_unbox.slow.147.callee_unbox.merge.148_crit_edge ]
  %r621 = phi i64 [ %r589, %logical.then.145 ], [ %r619, %callee_unbox.slow.147.callee_unbox.merge.148_crit_edge ]
  %37 = trunc i64 %r621 to i32
  %c1.i69 = inttoptr i32 %37 to ptr
  %r.i7071 = invoke double @js_closure_call1(ptr %c1.i69, i64 9222246136947933185, double %r1357)
          to label %eh.cont623 unwind label %wasm.eh.dispatch.114

eh.cont623:                                       ; preds = %callee_unbox.merge.148
  %r624 = bitcast double %r.i7071 to i64
  %r625 = and i64 %r624, 9221120237041090560
  %r626.not = icmp eq i64 %r625, 9221120237041090560
  br i1 %r626.not, label %truthy.tag.150, label %truthy.num.149

truthy.num.149:                                   ; preds = %eh.cont623
  %r627 = fcmp ueq double %r.i7071, 0.000000e+00
  br i1 %r627, label %logical.merge.155, label %logical.then.154

truthy.tag.150:                                   ; preds = %eh.cont623
  %r629 = add i64 %r624, -9222246136947933185
  %r630 = icmp ult i64 %r629, 4
  br i1 %r630, label %logical.merge.139, label %truthy.obj.153

truthy.slow.151:                                  ; preds = %truthy.obj.153
  %r636 = invoke i32 @js_is_truthy(double %r.i7071)
          to label %eh.cont637 unwind label %wasm.eh.dispatch.114

eh.cont637:                                       ; preds = %truthy.slow.151
  %r638.not = icmp eq i32 %r636, 0
  br i1 %r638.not, label %logical.merge.155, label %logical.then.154

truthy.obj.153:                                   ; preds = %truthy.tag.150
  %r631.mask = and i64 %r624, -281474976710656
  %r632 = icmp eq i64 %r631.mask, 9222527611924643840
  %r633 = and i64 %r624, 281474976710655
  %r634 = icmp ne i64 %r633, 0
  %r635 = and i1 %r632, %r634
  br i1 %r635, label %logical.then.154, label %truthy.slow.151

logical.then.154:                                 ; preds = %eh.cont603, %truthy.num.140, %truthy.num.149, %eh.cont637, %truthy.merge.143, %truthy.obj.153, %logical.merge.139
  %r644 = load i64, ptr %r10, align 8
  %38 = trunc i64 %r644 to i32
  %39 = add i32 %38, 56
  %r646 = inttoptr i32 %39 to ptr
  %r64716 = load double, ptr %r646, align 8
  store double %r64716, ptr %r67, align 8
  call void @js_shadow_slot_bind(i32 37, ptr nonnull %r67)
  %r652 = load i64, ptr %r10, align 8
  %40 = trunc i64 %r652 to i32
  %41 = add i32 %40, 24
  %r654 = inttoptr i32 %41 to ptr
  %r655 = load i64, ptr %r654, align 8
  %r656 = bitcast i64 %r655 to double
  %r657 = and i64 %r655, 9221120237041090560
  %r658.not = icmp eq i64 %r657, 9221120237041090560
  br i1 %r658.not, label %truthy.tag.157, label %truthy.num.156

logical.merge.155:                                ; preds = %eh.cont703, %truthy.num.168, %truthy.num.149, %eh.cont637, %eh.cont583, %truthy.num.133, %truthy.obj.172, %truthy.merge.136, %logical.then.173, %truthy.merge.171, %logical.merge.139
  call void @js_shadow_slot_set(i32 14, i64 0)
  %r737 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r738.not = icmp eq i32 %r737, 0
  br i1 %r738.not, label %gcpoll.done.179, label %gcpoll.178

truthy.num.156:                                   ; preds = %logical.then.154
  %r659 = fcmp ueq double %r656, 0.000000e+00
  br i1 %r659, label %ternary.else.162, label %ternary.then.161

truthy.tag.157:                                   ; preds = %logical.then.154
  %r661 = add i64 %r655, -9222246136947933185
  %r662 = icmp ult i64 %r661, 4
  br i1 %r662, label %truthy.merge.159, label %truthy.obj.160

truthy.slow.158:                                  ; preds = %truthy.obj.160
  %r668 = invoke i32 @js_is_truthy(double %r656)
          to label %eh.cont669 unwind label %wasm.eh.dispatch.114

eh.cont669:                                       ; preds = %truthy.slow.158
  %r670.not = icmp eq i32 %r668, 0
  br i1 %r670.not, label %ternary.else.162, label %ternary.then.161

truthy.merge.159:                                 ; preds = %truthy.tag.157
  %r660 = icmp eq i64 %r655, 9222246136947933188
  br i1 %r660, label %ternary.then.161, label %ternary.else.162

truthy.obj.160:                                   ; preds = %truthy.tag.157
  %r663.mask = and i64 %r655, -281474976710656
  %r664 = icmp eq i64 %r663.mask, 9222527611924643840
  %r665 = and i64 %r655, 281474976710655
  %r666 = icmp ne i64 %r665, 0
  %r667 = and i1 %r664, %r666
  br i1 %r667, label %ternary.then.161, label %truthy.slow.158

ternary.then.161:                                 ; preds = %eh.cont669, %truthy.num.156, %truthy.obj.160, %truthy.merge.159
  %r672 = load double, ptr %r563, align 8
  br label %ternary.merge.163

ternary.else.162:                                 ; preds = %eh.cont669, %truthy.num.156, %truthy.merge.159
  %r673 = load double, ptr %r563, align 8
  %r.i3031 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347286, double %r673, i64 range(i64 9221120237041090560, 0) %r677, ptr null, i32 0)
          to label %ternary.merge.163 unwind label %wasm.eh.dispatch.114

ternary.merge.163:                                ; preds = %ternary.else.162, %ternary.then.161
  %r681 = phi double [ %r672, %ternary.then.161 ], [ %r.i3031, %ternary.else.162 ]
  %r682 = load double, ptr %r67, align 8
  store double %r681, ptr %r685, align 8
  %r.i3334 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347287, double %r682, i64 range(i64 9221120237041090560, 0) %r684, ptr nonnull %r685, i32 1)
          to label %idispatch.merge.167 unwind label %wasm.eh.dispatch.114

idispatch.merge.167:                              ; preds = %ternary.merge.163
  store double 0.000000e+00, ptr %r67, align 8
  call void @js_shadow_slot_set(i32 37, i64 0)
  %r690 = bitcast double %r.i3334 to i64
  %r691 = and i64 %r690, 9221120237041090560
  %r692.not = icmp eq i64 %r691, 9221120237041090560
  br i1 %r692.not, label %truthy.tag.169, label %truthy.num.168

truthy.num.168:                                   ; preds = %idispatch.merge.167
  %r693 = fcmp ueq double %r.i3334, 0.000000e+00
  br i1 %r693, label %logical.then.173, label %logical.merge.155

truthy.tag.169:                                   ; preds = %idispatch.merge.167
  %r695 = add i64 %r690, -9222246136947933185
  %r696 = icmp ult i64 %r695, 4
  br i1 %r696, label %truthy.merge.171, label %truthy.obj.172

truthy.slow.170:                                  ; preds = %truthy.obj.172
  %r702 = invoke i32 @js_is_truthy(double %r.i3334)
          to label %eh.cont703 unwind label %wasm.eh.dispatch.114

eh.cont703:                                       ; preds = %truthy.slow.170
  %r704.not = icmp eq i32 %r702, 0
  br i1 %r704.not, label %logical.then.173, label %logical.merge.155

truthy.merge.171:                                 ; preds = %truthy.tag.169
  %r694 = icmp eq i64 %r690, 9222246136947933188
  br i1 %r694, label %logical.merge.155, label %logical.then.173

truthy.obj.172:                                   ; preds = %truthy.tag.169
  %r697.mask = and i64 %r690, -281474976710656
  %r698 = icmp eq i64 %r697.mask, 9222527611924643840
  %r699 = and i64 %r690, 281474976710655
  %r700 = icmp ne i64 %r699, 0
  %r701 = and i1 %r698, %r700
  br i1 %r701, label %logical.merge.155, label %truthy.slow.170

logical.then.173:                                 ; preds = %eh.cont703, %truthy.num.168, %truthy.merge.171
  %r710 = load i64, ptr %r10, align 8
  %42 = trunc i64 %r710 to i32
  %43 = add i32 %42, 88
  %r712 = inttoptr i32 %43 to ptr
  %r713 = load i64, ptr %r712, align 8
  %44 = trunc i64 %r713 to i32
  %r715 = inttoptr i32 %44 to ptr
  store i64 9222246136947933188, ptr %r715, align 8
  br label %logical.merge.155

gcpoll.178:                                       ; preds = %logical.merge.155
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.179 unwind label %wasm.eh.dispatch.114

gcpoll.done.179:                                  ; preds = %gcpoll.178, %logical.merge.155
  %r741 = fadd double %r481.0, 1.000000e+00
  br label %for.cond.118

try.catch.fail.181:                               ; preds = %wasm.eh.accepted.184
  %r776 = call double @js_get_exception()
  store double %r776, ptr %r777, align 8
  call void @js_shadow_slot_bind(i32 46, ptr nonnull %r777)
  store volatile double 0x7FFC000000000001, ptr %r21, align 8
  store double 0x7FFC000000000001, ptr %r487, align 8
  call void @js_shadow_slot_set(i32 13, i64 0)
  %r806 = load double, ptr %r777, align 8
  store double 0.000000e+00, ptr %r777, align 8
  call void @js_throw(double %r806)
  unreachable

wasm.eh.dispatch.182:                             ; preds = %if.merge.191, %eh.cont772, %if.then.189
  %r745 = catchswitch within none [label %wasm.eh.pad.183] unwind to caller

wasm.eh.pad.183:                                  ; preds = %wasm.eh.dispatch.182
  %r746 = catchpad within %r745 [ptr null]
  %r747 = call ptr @llvm.wasm.get.exception(token %r746)
  %r748 = call i1 @js_wasm_eh_is_perry(ptr %r747) [ "funclet"(token %r746) ]
  br i1 %r748, label %wasm.eh.accepted.184, label %wasm.eh.foreign.185

wasm.eh.accepted.184:                             ; preds = %wasm.eh.pad.183
  call void @js_wasm_eh_pad_enter(ptr nonnull %r744) [ "funclet"(token %r746) ]
  catchret from %r746 to label %try.catch.fail.181

wasm.eh.foreign.185:                              ; preds = %wasm.eh.pad.183
  call void @llvm.wasm.rethrow() [ "funclet"(token %r746) ]
  unreachable

if.then.189:                                      ; preds = %try.catch.112
  %r765 = fadd double %r481.0, 1.000000e+00
  store double %r765, ptr %r24, align 8
  store double 0.000000e+00, ptr %r25, align 8
  %r770 = load double, ptr %r743, align 8
  store volatile double %r770, ptr %r27, align 8
  %r771 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r21, i32 4)
          to label %eh.cont772 unwind label %wasm.eh.dispatch.182

eh.cont772:                                       ; preds = %if.then.189
  invoke void @js_throw(double %r771)
          to label %eh.cont773 unwind label %wasm.eh.dispatch.182

eh.cont773:                                       ; preds = %eh.cont772
  unreachable

if.merge.191:                                     ; preds = %try.catch.112
  %r774 = load double, ptr %r743, align 8
  invoke void @js_throw(double %r774)
          to label %eh.cont775 unwind label %wasm.eh.dispatch.182

eh.cont775:                                       ; preds = %if.merge.191
  unreachable

switch.early.test1:                               ; preds = %if.then.207
  %trunc = trunc nuw i64 %r854 to i16
  switch i16 %trunc, label %scope_set.barrier.done.212 [
    i16 32767, label %scope_set.barrier.maybe.210
    i16 32765, label %scope_set.barrier.maybe.210
    i16 32762, label %scope_set.barrier.maybe.210
  ]

if.then.207:                                      ; preds = %gcpoll.done.130
  store volatile double 0x7FFC000000000001, ptr %r21, align 8
  store double 0x7FFC000000000001, ptr %r487, align 8
  call void @js_shadow_slot_set(i32 13, i64 0)
  %r835 = load i64, ptr %r10, align 8
  %45 = trunc i64 %r835 to i32
  %46 = add i32 %45, 72
  %r837 = inttoptr i32 %46 to ptr
  %r838 = load i64, ptr %r837, align 8
  %47 = trunc i64 %r838 to i32
  %48 = add i32 %47, 8
  %r841 = inttoptr i32 %48 to ptr
  %r8428 = load double, ptr %r841, align 8
  %r844 = call double @js_to_numeric(double %r8428)
  %r845 = call double @js_numeric_step(double %r844, i32 0)
  %r846 = bitcast double %r845 to i64
  %r847 = load i64, ptr %r10, align 8
  %49 = trunc i64 %r847 to i32
  %50 = add i32 %49, 72
  %r849 = inttoptr i32 %50 to ptr
  %r850 = load i64, ptr %r849, align 8
  %r851 = and i64 %r850, 281474976710655
  %r852 = add nuw nsw i64 %r851, 8
  %51 = trunc i64 %r852 to i32
  %r853 = inttoptr i32 %51 to ptr
  store double %r845, ptr %r853, align 8
  %r854 = lshr i64 %r846, 48
  %r855 = icmp eq i64 %r854, 0
  %r856 = icmp ugt i64 %r846, 4095
  %r857 = and i1 %r856, %r855
  %52 = freeze i1 %r857
  br i1 %52, label %scope_set.barrier.maybe.210, label %switch.early.test1

scope_set.barrier.maybe.210:                      ; preds = %switch.early.test1, %switch.early.test1, %switch.early.test1, %if.then.207
  %53 = trunc i64 %r850 to i32
  %54 = add i32 %53, -7
  %r865 = inttoptr i32 %54 to ptr
  %r866 = load i8, ptr %r865, align 1
  %r867 = and i8 %r866, 34
  %r868 = icmp ne i8 %r867, 2
  %r869 = load atomic i32, ptr @PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT monotonic, align 4
  %r870 = icmp ne i32 %r869, 0
  %r871 = or i1 %r868, %r870
  br i1 %r871, label %scope_set.barrier.211, label %scope_set.barrier.done.212

scope_set.barrier.211:                            ; preds = %scope_set.barrier.maybe.210
  call void @js_write_barrier_slot_validated_parent(i64 %r851, i64 %r852, i64 %r846)
  br label %scope_set.barrier.done.212

scope_set.barrier.done.212:                       ; preds = %switch.early.test1, %scope_set.barrier.211, %scope_set.barrier.maybe.210
  %r872 = load double, ptr %r8, align 8
  store volatile double %r872, ptr %r28, align 8
  %r875 = load volatile double, ptr %r28, align 8
  store volatile double %r875, ptr %r28, align 8
  %r876 = call i32 @js_array_record_stack_enter(ptr nonnull %r28, i32 3, i32 0)
  %r877 = icmp ne i32 %r876, 0
  %r884 = uitofp i1 %r877 to double
  store double %r884, ptr %r33, align 8
  %r885 = load volatile double, ptr %r28, align 8
  store double 0x7FFC000000000001, ptr %r886, align 8
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r888)
  %r975 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.318.dispatch to i64), 9221120237041090560
  %r1011 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.312.dispatch to i64), 9221120237041090560
  %r1014 = getelementptr inbounds nuw i8, ptr %r1012, i32 8
  br label %for.cond.220

try.catch.214:                                    ; preds = %wasm.eh.accepted.218
  %r1094 = call double @js_catch_enter()
  store double %r1094, ptr %r1095, align 8
  call void @js_shadow_slot_bind(i32 22, ptr nonnull %r1095)
  call void @js_wasm_eh_checkpoint_save(ptr nonnull %r1096)
  br i1 %r887.0, label %if.then.275, label %if.merge.277

wasm.eh.dispatch.216:                             ; preds = %alen.slow.262, %scope_set.barrier.done.261, %logical.then.254, %record.next.guard.224, %65, %str_addref.done.234, %gcpoll.264, %scope_set.barrier.260, %truthy.slow.251, %truthy.slow.242, %str_addref.call.233, %gcpoll.231, %record.next.cold.229
  %r887.0 = phi i1 [ true, %gcpoll.264 ], [ true, %alen.slow.262 ], [ true, %scope_set.barrier.done.261 ], [ true, %scope_set.barrier.260 ], [ true, %logical.then.254 ], [ true, %truthy.slow.251 ], [ true, %65 ], [ true, %truthy.slow.242 ], [ true, %str_addref.done.234 ], [ true, %str_addref.call.233 ], [ false, %gcpoll.231 ], [ false, %record.next.cold.229 ], [ false, %record.next.guard.224 ]
  %r889 = catchswitch within none [label %wasm.eh.pad.217] unwind to caller

wasm.eh.pad.217:                                  ; preds = %wasm.eh.dispatch.216
  %r890 = catchpad within %r889 [ptr null]
  %r891 = call ptr @llvm.wasm.get.exception(token %r890)
  %r892 = call i1 @js_wasm_eh_is_perry(ptr %r891) [ "funclet"(token %r890) ]
  br i1 %r892, label %wasm.eh.accepted.218, label %wasm.eh.foreign.219

wasm.eh.accepted.218:                             ; preds = %wasm.eh.pad.217
  call void @js_wasm_eh_pad_enter(ptr nonnull %r888) [ "funclet"(token %r890) ]
  catchret from %r890 to label %try.catch.214

wasm.eh.foreign.219:                              ; preds = %wasm.eh.pad.217
  call void @llvm.wasm.rethrow() [ "funclet"(token %r890) ]
  unreachable

for.cond.220:                                     ; preds = %gcpoll.done.265, %scope_set.barrier.done.212
  %r880.0 = phi double [ 0.000000e+00, %scope_set.barrier.done.212 ], [ %r1093, %gcpoll.done.265 ]
  %r897 = load volatile double, ptr %r28, align 8
  br i1 %r877, label %record.next.cold.229, label %record.next.guard.224

for.body.221:                                     ; preds = %gcpoll.done.232
  %r963 = load double, ptr %r886, align 8
  %r964 = bitcast double %r963 to i64
  %r965 = and i64 %r964, -281474976710656
  %r966.not = icmp eq i64 %r965, 9223090561878065152
  br i1 %r966.not, label %str_addref.call.233, label %str_addref.done.234

record.next.guard.224:                            ; preds = %for.cond.220
  %r907 = bitcast double %r897 to i64
  %55 = trunc i64 %r907 to i32
  %56 = add i32 %55, -8
  %r910 = inttoptr i32 %56 to ptr
  %r911 = load i32, ptr %r910, align 4
  %c1.i48 = inttoptr i32 %55 to ptr
  %r.i4950 = invoke i32 @perry_array_element_kind(ptr %c1.i48)
          to label %eh.cont915 unwind label %wasm.eh.dispatch.216

eh.cont915:                                       ; preds = %record.next.guard.224
  %r912 = and i32 %r911, 33023
  %r913 = icmp eq i32 %r912, 1
  %r916.not = icmp eq i32 %r.i4950, 0
  %r918 = and i1 %r913, %r916.not
  %r919 = load i8, ptr @PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED, align 1
  %r920 = and i8 %r919, 1
  %r921 = icmp eq i8 %r920, 0
  %r922 = and i1 %r918, %r921
  br i1 %r922, label %record.next.bounds.225, label %record.next.cold.229

record.next.bounds.225:                           ; preds = %eh.cont915
  %r923 = fptoui double %r880.0 to i32
  %r1354 = load volatile double, ptr %r28, align 8
  %r1350 = load volatile double, ptr %r28, align 8
  %r1351 = bitcast double %r1350 to i64
  %57 = trunc i64 %r1351 to i32
  %r1353 = inttoptr i32 %57 to ptr
  %r925 = load i32, ptr %r1353, align 4
  %r926 = icmp ugt i32 %r925, %r923
  %r1346 = load volatile double, ptr %r28, align 8
  %r1347 = bitcast double %r1346 to i64
  %58 = trunc i64 %r1347 to i32
  %r1349 = inttoptr i32 %58 to ptr
  %r927 = getelementptr i8, ptr %r1349, i32 4
  %r928 = load i32, ptr %r927, align 4
  %r929 = icmp ugt i32 %r928, %r923
  %r930 = and i1 %r926, %r929
  br i1 %r930, label %record.next.load.227, label %record.next.unloadable.226

record.next.unloadable.226:                       ; preds = %record.next.bounds.225
  br i1 %r926, label %record.next.cold.229, label %record.next.merge.230

record.next.load.227:                             ; preds = %record.next.bounds.225
  %r931 = zext i32 %r923 to i64
  %r932 = shl nuw nsw i64 %r931, 3
  %r1343 = load volatile double, ptr %r28, align 8
  %r1339 = load volatile double, ptr %r28, align 8
  %r1334 = load volatile double, ptr %r28, align 8
  %r1335 = bitcast double %r1334 to i64
  %59 = trunc i64 %r1335 to i32
  %60 = add i32 %59, 4
  %r1338 = inttoptr i32 %60 to ptr
  %r935 = load i32, ptr %r1338, align 4
  %r1331 = load volatile double, ptr %r28, align 8
  %r1332 = bitcast double %r1331 to i64
  %61 = trunc i64 %r1332 to i32
  %62 = add i32 %61, -4
  %r937 = inttoptr i32 %62 to ptr
  %r938 = load i32, ptr %r937, align 4
  %r939 = zext i32 %r938 to i64
  %r940 = zext i32 %r935 to i64
  %r1328 = load volatile double, ptr %r28, align 8
  %r1329 = bitcast double %r1328 to i64
  %r942 = add nuw nsw i64 %r932, 4294967288
  %63 = shl nuw nsw i64 %r940, 3
  %r943 = sub nsw i64 %r942, %63
  %r944 = add nsw i64 %r943, %r939
  %r945 = add i64 %r944, %r1329
  %64 = trunc i64 %r945 to i32
  %r946 = inttoptr i32 %64 to ptr
  %r947 = load double, ptr %r946, align 8
  %r948 = bitcast double %r947 to i64
  %r949 = icmp eq i64 %r948, 9222246136947933200
  %r951 = select i1 %r949, double 0x7FFC000000000001, double %r947
  br label %record.next.merge.230

record.next.cold.229:                             ; preds = %record.next.unloadable.226, %eh.cont915, %for.cond.220
  store double %r880.0, ptr %r31, align 8
  %r952 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r28, i32 6)
          to label %eh.cont953 unwind label %wasm.eh.dispatch.216

eh.cont953:                                       ; preds = %record.next.cold.229
  %r954 = bitcast double %r952 to i64
  %r955 = icmp eq i64 %r954, 9222246136947933187
  %r956 = load volatile double, ptr %r34, align 8
  store volatile double 0x7FFC000000000001, ptr %r34, align 8
  br label %record.next.merge.230

record.next.merge.230:                            ; preds = %record.next.unloadable.226, %eh.cont953, %record.next.load.227
  %r957 = phi double [ %r951, %record.next.load.227 ], [ %r956, %eh.cont953 ], [ 0x7FFC000000000001, %record.next.unloadable.226 ]
  %r958 = phi i1 [ true, %record.next.load.227 ], [ %r955, %eh.cont953 ], [ false, %record.next.unloadable.226 ]
  store double %r957, ptr %r886, align 8
  call void @js_shadow_slot_bind(i32 19, ptr nonnull %r886)
  %r959 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r960.not = icmp eq i32 %r959, 0
  br i1 %r960.not, label %gcpoll.done.232, label %gcpoll.231

gcpoll.231:                                       ; preds = %record.next.merge.230
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.232 unwind label %wasm.eh.dispatch.216

gcpoll.done.232:                                  ; preds = %gcpoll.231, %record.next.merge.230
  br i1 %r958, label %for.body.221, label %if.then.293

str_addref.call.233:                              ; preds = %for.body.221
  invoke void @js_string_addref_if_heap_string(double %r963)
          to label %str_addref.call.233.str_addref.done.234_crit_edge unwind label %wasm.eh.dispatch.216

str_addref.call.233.str_addref.done.234_crit_edge: ; preds = %str_addref.call.233
  %r1327.pre = load double, ptr %r886, align 8
  br label %str_addref.done.234

str_addref.done.234:                              ; preds = %str_addref.call.233.str_addref.done.234_crit_edge, %for.body.221
  %r1327 = phi double [ %r1327.pre, %str_addref.call.233.str_addref.done.234_crit_edge ], [ %r963, %for.body.221 ]
  store double %r1327, ptr %r962, align 8
  call void @js_shadow_slot_bind(i32 20, ptr nonnull %r962)
  %r971 = load double, ptr %r962, align 8
  %r.i3637 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347288, double %r971, i64 range(i64 9221120237041090560, 0) %r975, ptr null, i32 0)
          to label %idispatch.merge.239 unwind label %wasm.eh.dispatch.216

idispatch.merge.239:                              ; preds = %str_addref.done.234
  %r.i3637.fr = freeze double %r.i3637
  %r979 = bitcast double %r.i3637.fr to i64
  %r980 = and i64 %r979, 9221120237041090560
  %r981.not = icmp eq i64 %r980, 9221120237041090560
  br i1 %r981.not, label %truthy.tag.241, label %truthy.num.240

truthy.num.240:                                   ; preds = %idispatch.merge.239
  %r982 = fcmp ueq double %r.i3637.fr, 0.000000e+00
  %r995107 = load double, ptr %r962, align 8
  br i1 %r982, label %65, label %truthy.merge.243.thread

truthy.tag.241:                                   ; preds = %idispatch.merge.239
  %r984 = add i64 %r979, -9222246136947933185
  %r985 = icmp ult i64 %r984, 4
  br i1 %r985, label %truthy.merge.243, label %truthy.obj.244

truthy.slow.242:                                  ; preds = %truthy.obj.244
  %r991 = invoke i32 @js_is_truthy(double %r.i3637.fr)
          to label %eh.cont992 unwind label %wasm.eh.dispatch.216

eh.cont992:                                       ; preds = %truthy.slow.242
  %r991.fr = freeze i32 %r991
  %r993.not = icmp eq i32 %r991.fr, 0
  %r995109 = load double, ptr %r962, align 8
  br i1 %r993.not, label %65, label %truthy.merge.243.thread

truthy.merge.243:                                 ; preds = %truthy.tag.241
  %r983 = icmp eq i64 %r979, 9222246136947933188
  %r995 = load double, ptr %r962, align 8
  br i1 %r983, label %truthy.merge.243.thread, label %65

truthy.merge.243.thread:                          ; preds = %truthy.obj.244, %eh.cont992, %truthy.num.240, %truthy.merge.243
  br label %65

65:                                               ; preds = %eh.cont992, %truthy.num.240, %truthy.merge.243, %truthy.merge.243.thread
  %66 = phi double [ %r.i3637.fr, %truthy.merge.243.thread ], [ %r995, %truthy.merge.243 ], [ %r995107, %truthy.num.240 ], [ %r995109, %eh.cont992 ]
  store double %66, ptr %r970, align 8
  call void @js_shadow_slot_bind(i32 21, ptr nonnull %r970)
  %r997 = load double, ptr %r970, align 8
  %r1000 = load i64, ptr %r10, align 8
  %67 = trunc i64 %r1000 to i32
  %68 = add i32 %67, 64
  %r1002 = inttoptr i32 %68 to ptr
  %r100313 = load double, ptr %r1002, align 8
  %69 = add i32 %67, 48
  %r1007 = inttoptr i32 %69 to ptr
  %r100814 = load double, ptr %r1007, align 8
  store double %r100313, ptr %r1012, align 8
  store double %r100814, ptr %r1014, align 8
  %r.i3940 = invoke double @js_typed_feedback_native_call_method_by_id(i64 4905773613279347289, double %r997, i64 range(i64 9221120237041090560, 0) %r1011, ptr nonnull %r1012, i32 2)
          to label %idispatch.merge.248 unwind label %wasm.eh.dispatch.216

truthy.obj.244:                                   ; preds = %truthy.tag.241
  %r986.mask = and i64 %r979, -281474976710656
  %r987 = icmp eq i64 %r986.mask, 9222527611924643840
  %r988 = and i64 %r979, 281474976710655
  %r989 = icmp ne i64 %r988, 0
  %r990 = and i1 %r987, %r989
  br i1 %r990, label %truthy.merge.243.thread, label %truthy.slow.242

idispatch.merge.248:                              ; preds = %65
  %r1018 = bitcast double %r.i3940 to i64
  %r1019 = and i64 %r1018, 9221120237041090560
  %r1020.not = icmp eq i64 %r1019, 9221120237041090560
  br i1 %r1020.not, label %truthy.tag.250, label %truthy.num.249

truthy.num.249:                                   ; preds = %idispatch.merge.248
  %r1021 = fcmp ueq double %r.i3940, 0.000000e+00
  br i1 %r1021, label %logical.merge.255, label %logical.then.254

truthy.tag.250:                                   ; preds = %idispatch.merge.248
  %r1023 = add i64 %r1018, -9222246136947933185
  %r1024 = icmp ult i64 %r1023, 4
  br i1 %r1024, label %truthy.merge.252, label %truthy.obj.253

truthy.slow.251:                                  ; preds = %truthy.obj.253
  %r1030 = invoke i32 @js_is_truthy(double %r.i3940)
          to label %eh.cont1031 unwind label %wasm.eh.dispatch.216

eh.cont1031:                                      ; preds = %truthy.slow.251
  %r1032.not = icmp eq i32 %r1030, 0
  br i1 %r1032.not, label %logical.merge.255, label %logical.then.254

truthy.merge.252:                                 ; preds = %truthy.tag.250
  %r1022 = icmp eq i64 %r1018, 9222246136947933188
  br i1 %r1022, label %logical.then.254, label %logical.merge.255

truthy.obj.253:                                   ; preds = %truthy.tag.250
  %r1025.mask = and i64 %r1018, -281474976710656
  %r1026 = icmp eq i64 %r1025.mask, 9222527611924643840
  %r1027 = and i64 %r1018, 281474976710655
  %r1028 = icmp ne i64 %r1027, 0
  %r1029 = and i1 %r1026, %r1028
  br i1 %r1029, label %logical.then.254, label %truthy.slow.251

logical.then.254:                                 ; preds = %eh.cont1031, %truthy.num.249, %truthy.obj.253, %truthy.merge.252
  %r1034 = load double, ptr %r970, align 8
  %r1036 = load i64, ptr %r10, align 8
  %70 = trunc i64 %r1036 to i32
  %71 = add i32 %70, 72
  %r1038 = inttoptr i32 %71 to ptr
  %r1039 = load i64, ptr %r1038, align 8
  %72 = trunc i64 %r1039 to i32
  %r1041 = inttoptr i32 %72 to ptr
  %r1042 = load i64, ptr %r1041, align 8
  %73 = trunc i64 %r1042 to i32
  %c1.i65 = inttoptr i32 %73 to ptr
  %r.i6667 = invoke i64 @js_array_push_f64_spec_or_own(ptr %c1.i65, double %r1034, ptr nonnull %r1045)
          to label %eh.cont1047 unwind label %wasm.eh.dispatch.216

eh.cont1047:                                      ; preds = %logical.then.254
  %r1048 = load i32, ptr %r1045, align 4
  %r1049.not = icmp eq i32 %r1048, 0
  br i1 %r1049.not, label %apush.own.builtin.258, label %logical.merge.255, !prof !0

logical.merge.255:                                ; preds = %alen.slow.262, %eh.cont1031, %truthy.num.249, %eh.cont1080, %eh.cont1047, %truthy.merge.252
  call void @js_shadow_slot_set(i32 20, i64 0)
  call void @js_shadow_slot_set(i32 21, i64 0)
  %r1089 = load volatile i32, ptr @PERRY_GC_POLL_ARMED, align 4
  %r1090.not = icmp eq i32 %r1089, 0
  br i1 %r1090.not, label %gcpoll.done.265, label %gcpoll.264

apush.own.builtin.258:                            ; preds = %eh.cont1047
  %r1052 = or i64 %r.i6667, 9222527611924643840
  %r1054 = load i64, ptr %r10, align 8
  %74 = trunc i64 %r1054 to i32
  %75 = add i32 %74, 72
  %r1056 = inttoptr i32 %75 to ptr
  %r1057 = load i64, ptr %r1056, align 8
  %r1058 = and i64 %r1057, 281474976710655
  %76 = trunc i64 %r1057 to i32
  %r1059 = inttoptr i32 %76 to ptr
  store i64 %r1052, ptr %r1059, align 8
  %r1060 = lshr i64 %r1052, 48
  %trunc15 = trunc nuw i64 %r1060 to i16
  switch i16 %trunc15, label %scope_set.barrier.done.261 [
    i16 32767, label %scope_set.barrier.maybe.259
    i16 32765, label %scope_set.barrier.maybe.259
  ]

scope_set.barrier.maybe.259:                      ; preds = %apush.own.builtin.258, %apush.own.builtin.258
  %77 = add i32 %76, -7
  %r1071 = inttoptr i32 %77 to ptr
  %r1072 = load i8, ptr %r1071, align 1
  %r1073 = and i8 %r1072, 34
  %r1074 = icmp ne i8 %r1073, 2
  %r1075 = load atomic i32, ptr @PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT monotonic, align 4
  %r1076 = icmp ne i32 %r1075, 0
  %r1077 = or i1 %r1074, %r1076
  br i1 %r1077, label %scope_set.barrier.260, label %scope_set.barrier.done.261

scope_set.barrier.260:                            ; preds = %scope_set.barrier.maybe.259
  invoke void @js_write_barrier_slot_validated_parent(i64 %r1058, i64 %r1058, i64 %r1052)
          to label %scope_set.barrier.done.261 unwind label %wasm.eh.dispatch.216

scope_set.barrier.done.261:                       ; preds = %scope_set.barrier.260, %apush.own.builtin.258, %scope_set.barrier.maybe.259
  %78 = trunc i64 %r.i6667 to i32
  %c1.i88 = inttoptr i32 %78 to ptr
  %r.i8990 = invoke i64 @js_array_length_leaf(ptr %c1.i88)
          to label %eh.cont1080 unwind label %wasm.eh.dispatch.216

eh.cont1080:                                      ; preds = %scope_set.barrier.done.261
  %r1081 = icmp slt i64 %r.i8990, 0
  br i1 %r1081, label %alen.slow.262, label %logical.merge.255

alen.slow.262:                                    ; preds = %eh.cont1080
  %r.i9697 = invoke i32 @js_array_length(ptr %c1.i88)
          to label %logical.merge.255 unwind label %wasm.eh.dispatch.216

gcpoll.264:                                       ; preds = %logical.merge.255
  invoke void @js_gc_loop_safepoint()
          to label %gcpoll.done.265 unwind label %wasm.eh.dispatch.216

gcpoll.done.265:                                  ; preds = %gcpoll.264, %logical.merge.255
  %r1093 = fadd double %r880.0, 1.000000e+00
  br label %for.cond.220

try.catch.fail.267:                               ; preds = %wasm.eh.accepted.270
  %r1128 = call double @js_get_exception()
  store double %r1128, ptr %r1129, align 8
  call void @js_shadow_slot_bind(i32 47, ptr nonnull %r1129)
  store volatile double 0x7FFC000000000001, ptr %r28, align 8
  store double 0x7FFC000000000001, ptr %r886, align 8
  call void @js_shadow_slot_set(i32 19, i64 0)
  %r1158 = load double, ptr %r1129, align 8
  store double 0.000000e+00, ptr %r1129, align 8
  call void @js_throw(double %r1158)
  unreachable

wasm.eh.dispatch.268:                             ; preds = %if.merge.277, %eh.cont1124, %if.then.275
  %r1097 = catchswitch within none [label %wasm.eh.pad.269] unwind to caller

wasm.eh.pad.269:                                  ; preds = %wasm.eh.dispatch.268
  %r1098 = catchpad within %r1097 [ptr null]
  %r1099 = call ptr @llvm.wasm.get.exception(token %r1098)
  %r1100 = call i1 @js_wasm_eh_is_perry(ptr %r1099) [ "funclet"(token %r1098) ]
  br i1 %r1100, label %wasm.eh.accepted.270, label %wasm.eh.foreign.271

wasm.eh.accepted.270:                             ; preds = %wasm.eh.pad.269
  call void @js_wasm_eh_pad_enter(ptr nonnull %r1096) [ "funclet"(token %r1098) ]
  catchret from %r1098 to label %try.catch.fail.267

wasm.eh.foreign.271:                              ; preds = %wasm.eh.pad.269
  call void @llvm.wasm.rethrow() [ "funclet"(token %r1098) ]
  unreachable

if.then.275:                                      ; preds = %try.catch.214
  %r1117 = fadd double %r880.0, 1.000000e+00
  store double %r1117, ptr %r31, align 8
  store double 0.000000e+00, ptr %r32, align 8
  %r1122 = load double, ptr %r1095, align 8
  store volatile double %r1122, ptr %r34, align 8
  %r1123 = invoke double @js_array_record_stack_dispatch(ptr nonnull %r28, i32 4)
          to label %eh.cont1124 unwind label %wasm.eh.dispatch.268

eh.cont1124:                                      ; preds = %if.then.275
  invoke void @js_throw(double %r1123)
          to label %eh.cont1125 unwind label %wasm.eh.dispatch.268

eh.cont1125:                                      ; preds = %eh.cont1124
  unreachable

if.merge.277:                                     ; preds = %try.catch.214
  %r1126 = load double, ptr %r1095, align 8
  invoke void @js_throw(double %r1126)
          to label %eh.cont1127 unwind label %wasm.eh.dispatch.268

eh.cont1127:                                      ; preds = %if.merge.277
  unreachable

if.then.293:                                      ; preds = %gcpoll.done.232
  store volatile double 0x7FFC000000000001, ptr %r28, align 8
  store double 0x7FFC000000000001, ptr %r886, align 8
  call void @js_shadow_slot_set(i32 19, i64 0)
  %r1187 = load i64, ptr %r10, align 8
  %79 = trunc i64 %r1187 to i32
  %80 = add i32 %79, 88
  %r1189 = inttoptr i32 %80 to ptr
  %r1190 = load i64, ptr %r1189, align 8
  %81 = trunc i64 %r1190 to i32
  %r1192 = inttoptr i32 %81 to ptr
  %r1193 = load i64, ptr %r1192, align 8
  %r1194 = bitcast i64 %r1193 to double
  %r1195 = and i64 %r1193, 9221120237041090560
  %r1196.not = icmp eq i64 %r1195, 9221120237041090560
  br i1 %r1196.not, label %truthy.tag.297, label %truthy.num.296

truthy.num.296:                                   ; preds = %if.then.293
  %r1197 = fcmp ueq double %r1194, 0.000000e+00
  br i1 %r1197, label %ternary.else.313, label %test.right.301

truthy.tag.297:                                   ; preds = %if.then.293
  %r1199 = add i64 %r1193, -9222246136947933185
  %r1200 = icmp ult i64 %r1199, 4
  br i1 %r1200, label %truthy.merge.299, label %truthy.obj.300

truthy.slow.298:                                  ; preds = %truthy.obj.300
  %r1206 = call i32 @js_is_truthy(double %r1194)
  %r1207.not = icmp eq i32 %r1206, 0
  br i1 %r1207.not, label %ternary.else.313, label %test.right.301

truthy.merge.299:                                 ; preds = %truthy.tag.297
  %r1198 = icmp eq i64 %r1193, 9222246136947933188
  br i1 %r1198, label %test.right.301, label %ternary.else.313

truthy.obj.300:                                   ; preds = %truthy.tag.297
  %r1201.mask = and i64 %r1193, -281474976710656
  %r1202 = icmp eq i64 %r1201.mask, 9222527611924643840
  %r1203 = and i64 %r1193, 281474976710655
  %r1204 = icmp ne i64 %r1203, 0
  %r1205 = and i1 %r1202, %r1204
  br i1 %r1205, label %test.right.301, label %truthy.slow.298

test.right.301:                                   ; preds = %truthy.slow.298, %truthy.num.296, %truthy.obj.300, %truthy.merge.299
  %82 = add i32 %79, 56
  %r1211 = inttoptr i32 %82 to ptr
  %r1212 = load i64, ptr %r1211, align 8
  %r1214 = add i64 %r1212, -9222527611926740992
  %r1215 = icmp ult i64 %r1214, 281474974613504
  br i1 %r1215, label %pget.recv_ok.303, label %pget.slow.304

pget.recv_ok.303:                                 ; preds = %test.right.301
  %r1217 = call ptr @perry_shape_dir_cell()
  %r12189 = load i64, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.319.handle, align 8
  %r1220 = call double @perry_probe_get(ptr %r1217, i64 %r1214, i64 %r12189, i64 1909965344552443787)
  %r1221 = bitcast double %r1220 to i64
  %r1222.not = icmp eq i64 %r1221, 9222246136947933200
  br i1 %r1222.not, label %pget.slow.304, label %pget.recv_merge.306

pget.slow.304:                                    ; preds = %test.right.301, %pget.recv_ok.303
  %r.i52 = call double @js_object_get_field_value(i64 %r1212, ptr nonnull getelementptr inbounds nuw (i8, ptr @perry_image_literal_0_666c6f77696e67, i32 8), i64 4905773613279347290)
  %.pre = bitcast double %r.i52 to i64
  br label %pget.recv_merge.306

pget.recv_merge.306:                              ; preds = %pget.slow.304, %pget.recv_ok.303
  %r1228.pre-phi = phi i64 [ %.pre, %pget.slow.304 ], [ %r1221, %pget.recv_ok.303 ]
  %r1227 = phi double [ %r.i52, %pget.slow.304 ], [ %r1220, %pget.recv_ok.303 ]
  %r1229 = and i64 %r1228.pre-phi, 9221120237041090560
  %r1230.not = icmp eq i64 %r1229, 9221120237041090560
  br i1 %r1230.not, label %truthy.tag.308, label %truthy.num.307

truthy.num.307:                                   ; preds = %pget.recv_merge.306
  %r1231 = fcmp ueq double %r1227, 0.000000e+00
  br i1 %r1231, label %ternary.then.312, label %ternary.else.313

truthy.tag.308:                                   ; preds = %pget.recv_merge.306
  %r1233 = add i64 %r1228.pre-phi, -9222246136947933185
  %r1234 = icmp ult i64 %r1233, 4
  br i1 %r1234, label %truthy.merge.310, label %truthy.obj.311

truthy.slow.309:                                  ; preds = %truthy.obj.311
  %r1240 = call i32 @js_is_truthy(double %r1227)
  %r1241.not = icmp eq i32 %r1240, 0
  br i1 %r1241.not, label %ternary.then.312, label %ternary.else.313

truthy.merge.310:                                 ; preds = %truthy.tag.308
  %r1232 = icmp eq i64 %r1228.pre-phi, 9222246136947933188
  br i1 %r1232, label %ternary.else.313, label %ternary.then.312

truthy.obj.311:                                   ; preds = %truthy.tag.308
  %r1235.mask = and i64 %r1228.pre-phi, -281474976710656
  %r1236 = icmp eq i64 %r1235.mask, 9222527611924643840
  %r1237 = and i64 %r1228.pre-phi, 281474976710655
  %r1238 = icmp ne i64 %r1237, 0
  %r1239 = and i1 %r1236, %r1238
  br i1 %r1239, label %ternary.else.313, label %truthy.slow.309

ternary.then.312:                                 ; preds = %truthy.slow.309, %truthy.num.307, %truthy.merge.310
  %r1245 = load i64, ptr %r10, align 8
  %83 = trunc i64 %r1245 to i32
  %84 = add i32 %83, 56
  %r1247 = inttoptr i32 %84 to ptr
  %r124811 = load double, ptr %r1247, align 8
  store double %r124811, ptr %r67, align 8
  call void @js_shadow_slot_bind(i32 37, ptr nonnull %r67)
  %r1250 = load double, ptr %r67, align 8
  %r1252 = or disjoint i64 ptrtoint (ptr @node_modules_glob_dist_commonjs_index_min_js_.str.298.dispatch to i64), 9221120237041090560
  %r1253 = call double @js_method_prepare(double %r1250, i64 %r1252)
  %r1254 = bitcast double %r1253 to i64
  %85 = and i64 %r1254, -3
  %r1256 = icmp eq i64 %85, 9222246136947933200
  %r1257 = select i1 %r1256, i64 9222246136947933185, i64 %r1254
  %r1258 = bitcast i64 %r1257 to double
  %r132312 = load i64, ptr %r15, align 8
  %86 = trunc i64 %r132312 to i32
  %r1326 = inttoptr i32 %86 to ptr
  %r1260 = load i64, ptr %r1326, align 8
  %r1261 = icmp eq i64 %r1260, 9222246136947933201
  br i1 %r1261, label %trusted_box.tdz.315, label %trusted_box.read.316

ternary.else.313:                                 ; preds = %truthy.slow.309, %truthy.num.307, %truthy.slow.298, %truthy.num.296, %truthy.obj.311, %truthy.merge.299, %truthy.merge.310
  %r1273 = load i64, ptr %r10, align 8
  %87 = trunc i64 %r1273 to i32
  %88 = add i32 %87, 80
  %r1275 = inttoptr i32 %88 to ptr
  %r1276 = load i64, ptr %r1275, align 8
  %89 = trunc i64 %r1276 to i32
  %90 = add i32 %89, 8
  %r1279 = inttoptr i32 %90 to ptr
  %r1280 = load i64, ptr %r1279, align 8
  %r1281 = icmp eq i64 %r1280, 9222246136947933201
  br i1 %r1281, label %scope_slot.tdz.320, label %scope_slot.read.321

trusted_box.tdz.315:                              ; preds = %ternary.then.312
  %r1262 = load double, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.243.handle, align 8
  %r.i73 = call i64 @js_box_get_bits_trusted_named(ptr nonnull %r1326, double %r1262)
  br label %trusted_box.read.316

trusted_box.read.316:                             ; preds = %trusted_box.tdz.315, %ternary.then.312
  %r1264 = phi i64 [ %r1260, %ternary.then.312 ], [ %r.i73, %trusted_box.tdz.315 ]
  %r1266 = load double, ptr %r67, align 8
  %r1319 = load double, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.320.handle, align 8
  store double %r1319, ptr %r1267, align 8
  %r1269 = getelementptr inbounds nuw i8, ptr %r1267, i32 8
  store i64 %r1264, ptr %r1269, align 8
  br i1 %r1256, label %lookup_first.by_name.317, label %lookup_first.value.318

lookup_first.by_name.317:                         ; preds = %trusted_box.read.316
  %r.i101 = call double @js_native_call_method_patched_proto(double %r1266, ptr nonnull @node_modules_glob_dist_commonjs_index_min_js_.str.298.bytes, i32 4, ptr nonnull %r1267, i32 2)
  br label %lookup_first.merge.319

lookup_first.value.318:                           ; preds = %trusted_box.read.316
  %r.i84 = call double @js_method_call_value(double %r1258, double %r1266, ptr nonnull %r1267, i32 2)
  br label %lookup_first.merge.319

lookup_first.merge.319:                           ; preds = %lookup_first.value.318, %lookup_first.by_name.317
  store double 0.000000e+00, ptr %r67, align 8
  call void @js_shadow_slot_set(i32 37, i64 0)
  br label %common.ret

scope_slot.tdz.320:                               ; preds = %ternary.else.313
  %r1282 = load double, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.321.handle, align 8
  %r.i75 = call i64 @js_box_get_bits_trusted_named(ptr nonnull %r1279, double %r1282)
  br label %scope_slot.read.321

scope_slot.read.321:                              ; preds = %scope_slot.tdz.320, %ternary.else.313
  %r1284 = phi i64 [ %r1280, %ternary.else.313 ], [ %r.i75, %scope_slot.tdz.320 ]
  %r1285 = bitcast i64 %r1284 to double
  %r1286 = and i64 %r1284, 9221120237041090560
  %r1287.not = icmp eq i64 %r1286, 9221120237041090560
  br i1 %r1287.not, label %truthy.tag.323, label %truthy.num.322

truthy.num.322:                                   ; preds = %scope_slot.read.321
  %r1288 = fcmp ueq double %r1285, 0.000000e+00
  br i1 %r1288, label %logical.then.327, label %common.ret

truthy.tag.323:                                   ; preds = %scope_slot.read.321
  %r1290 = add i64 %r1284, -9222246136947933185
  %r1291 = icmp ult i64 %r1290, 4
  br i1 %r1291, label %truthy.merge.325, label %truthy.obj.326

truthy.slow.324:                                  ; preds = %truthy.obj.326
  %r1297 = call i32 @js_is_truthy(double %r1285)
  %r1298.not = icmp eq i32 %r1297, 0
  br i1 %r1298.not, label %logical.then.327, label %common.ret

truthy.merge.325:                                 ; preds = %truthy.tag.323
  %r1289 = icmp eq i64 %r1284, 9222246136947933188
  br i1 %r1289, label %common.ret, label %logical.then.327

truthy.obj.326:                                   ; preds = %truthy.tag.323
  %r1292.mask = and i64 %r1284, -281474976710656
  %r1293 = icmp eq i64 %r1292.mask, 9222527611924643840
  %r1294 = and i64 %r1284, 281474976710655
  %r1295 = icmp ne i64 %r1294, 0
  %r1296 = and i1 %r1293, %r1295
  br i1 %r1296, label %common.ret, label %truthy.slow.324

logical.then.327:                                 ; preds = %truthy.slow.324, %truthy.num.322, %truthy.merge.325
  %r131510 = load i64, ptr %r15, align 8
  %91 = trunc i64 %r131510 to i32
  %r1318 = inttoptr i32 %91 to ptr
  %r1300 = load i64, ptr %r1318, align 8
  %r1301 = icmp eq i64 %r1300, 9222246136947933201
  br i1 %r1301, label %trusted_box.tdz.329, label %trusted_box.read.330

trusted_box.tdz.329:                              ; preds = %logical.then.327
  %r1302 = load double, ptr @node_modules_glob_dist_commonjs_index_min_js_.str.243.handle, align 8
  %r.i77 = call i64 @js_box_get_bits_trusted_named(ptr nonnull %r1318, double %r1302)
  br label %trusted_box.read.330

trusted_box.read.330:                             ; preds = %trusted_box.tdz.329, %logical.then.327
  %r1304 = phi i64 [ %r1300, %logical.then.327 ], [ %r.i77, %trusted_box.tdz.329 ]
  %92 = trunc i64 %r1304 to i32
  %c1.i102 = inttoptr i32 %92 to ptr
  %r.i103 = call double @perry_closure_node_modules_glob_dist_commonjs_index_min_js__619(ptr %c1.i102, i64 poison)
  br label %common.ret
}

; Function Attrs: nocallback nofree nounwind willreturn memory(argmem: write)
declare void @llvm.memset.p0.i64(ptr writeonly captures(none), i8, i64, i1 immarg) #6

attributes #0 = { mustprogress nofree willreturn memory(read) "target-features"="+exception-handling" }
attributes #1 = { "target-features"="+exception-handling" }
attributes #2 = { nocallback nofree nosync nounwind willreturn }
attributes #3 = { noreturn }
attributes #4 = { cold "target-features"="+exception-handling" }
attributes #5 = { optsize "target-features"="+exception-handling" }
attributes #6 = { nocallback nofree nounwind willreturn memory(argmem: write) }

!0 = !{!"branch_weights", !"expected", i32 2000, i32 1}
!1 = !{!"branch_weights", !"expected", i32 1, i32 2000}
