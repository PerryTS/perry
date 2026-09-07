; ModuleID = 'abi.56bc75269743c0fc-cgu.0'
source_filename = "abi.56bc75269743c0fc-cgu.0"
target datalayout = "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "arm64-apple-macosx11.0.0"

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define { i64, i32 } @pair(i32 noundef %bytes, i32 noundef %units, i32 noundef %source) unnamed_addr #0 !guid !4 {
start:
  %_8 = zext i32 %bytes to i64
  %_10 = zext i32 %units to i64
  %_9 = shl nuw i64 %_10, 32
  %_7 = or disjoint i64 %_9, %_8
  %0 = insertvalue { i64, i32 } poison, i64 %_7, 0
  %1 = insertvalue { i64, i32 } %0, i32 %source, 1
  ret { i64, i32 } %1
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable
define void @triple(ptr dead_on_unwind noalias nofree noundef writable writeonly sret([12 x i8]) align 4 captures(none) dereferenceable(12) initializes((0, 4)) %_0, i32 noundef %bytes, i32 noundef %units, i32 noundef %source) unnamed_addr #1 !guid !5 {
start:
  %.not = icmp eq i32 %bytes, 0
  br i1 %.not, label %bb2, label %bb4

bb4:                                              ; preds = %start
  %_4.sroa.5.0._0.sroa_idx = getelementptr inbounds nuw i8, ptr %_0, i64 4
  store i32 %units, ptr %_4.sroa.5.0._0.sroa_idx, align 4
  %_4.sroa.6.0._0.sroa_idx = getelementptr inbounds nuw i8, ptr %_0, i64 8
  store i32 %source, ptr %_4.sroa.6.0._0.sroa_idx, align 4
  br label %bb2

bb2:                                              ; preds = %start, %bb4
  store i32 %bytes, ptr %_0, align 4
  ret void
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define noundef range(i64 0, 12884901886) i64 @use_pair(i32 noundef %bytes, i32 noundef %units, i32 noundef %source) unnamed_addr #0 !guid !6 {
start:
  %0 = tail call { i64, i32 } @pair(i32 noundef %bytes, i32 noundef %units, i32 noundef %source) #3
  %1 = extractvalue { i64, i32 } %0, 0
  %.not = icmp eq i64 %1, 0
  %2 = extractvalue { i64, i32 } %0, 1
  %_7 = and i64 %1, 4294967295
  %_10 = lshr i64 %1, 32
  %_6 = add nuw nsw i64 %_7, %_10
  %_11 = zext i32 %2 to i64
  %3 = add nuw nsw i64 %_6, %_11
  %_0.sroa.0.0 = select i1 %.not, i64 0, i64 %3
  ret i64 %_0.sroa.0.0
}

; Function Attrs: mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable
define noundef range(i64 0, 12884901886) i64 @use_triple(i32 noundef %bytes, i32 noundef %units, i32 noundef %source) unnamed_addr #0 !guid !7 {
start:
  %_4 = alloca [12 x i8], align 4
  call void @llvm.lifetime.start.p0(ptr nonnull %_4)
  call void @triple(ptr noalias nofree noundef nonnull sret([12 x i8]) align 4 captures(none) dereferenceable(12) %_4, i32 noundef %bytes, i32 noundef %units, i32 noundef %source) #3
  %0 = load i32, ptr %_4, align 4, !noundef !8
  %.not = icmp eq i32 %0, 0
  %1 = getelementptr inbounds nuw i8, ptr %_4, i64 4
  %t1 = load i32, ptr %1, align 4
  %2 = getelementptr inbounds nuw i8, ptr %_4, i64 8
  %t2 = load i32, ptr %2, align 4
  %_7 = zext i32 %0 to i64
  %_9 = zext i32 %t1 to i64
  %_6 = add nuw nsw i64 %_9, %_7
  %_10 = zext i32 %t2 to i64
  %3 = add nuw nsw i64 %_6, %_10
  %_0.sroa.0.0 = select i1 %.not, i64 0, i64 %3
  call void @llvm.lifetime.end.p0(ptr nonnull %_4)
  ret i64 %_0.sroa.0.0
}

; Function Attrs: mustprogress nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.start.p0(ptr captures(none)) #2

; Function Attrs: mustprogress nocallback nofree nosync nounwind willreturn memory(argmem: readwrite)
declare void @llvm.lifetime.end.p0(ptr captures(none)) #2

attributes #0 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(none) uwtable "frame-pointer"="non-leaf" "probe-stack"="inline-asm" "target-cpu"="apple-m1" }
attributes #1 = { mustprogress nofree noinline norecurse nosync nounwind willreturn memory(argmem: write) uwtable "frame-pointer"="non-leaf" "probe-stack"="inline-asm" "target-cpu"="apple-m1" }
attributes #2 = { mustprogress nocallback nofree nosync nounwind willreturn memory(argmem: readwrite) }
attributes #3 = { noinline }

!llvm.module.flags = !{!0, !1, !2}
!llvm.ident = !{!3}

!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"uwtable", i32 2}
!2 = !{i32 7, !"frame-pointer", i32 1}
!3 = !{!"rustc version 1.100.0-nightly (f7d782a3b 2026-08-19)"}
!4 = !{i64 4859215404959254835}
!5 = !{i64 -5684407799154151066}
!6 = !{i64 7303624986867894911}
!7 = !{i64 1243954106977473866}
!8 = !{}
