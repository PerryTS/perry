; Runtime wasm catch shim. Checkpoint layout is supplied by perry-abi in build.rs.
; No JS executes in the funclet; Rust observes one ordinary return.
declare i32 @__gxx_wasm_personality_v0(...)
declare void @js_wasm_eh_checkpoint_save(ptr) nounwind
declare void @js_wasm_eh_pad_enter(ptr) nounwind
declare i1 @js_wasm_eh_is_perry(ptr) nounwind
declare ptr @llvm.wasm.get.exception(token)
declare void @llvm.wasm.rethrow()

define i32 @perry_trap_call(ptr %body, ptr %ctx) noinline personality ptr @__gxx_wasm_personality_v0 {
entry:
  %checkpoint = alloca [CHECKPOINT_BYTES x i8], align CHECKPOINT_ALIGN
  call void @js_wasm_eh_checkpoint_save(ptr %checkpoint)
  invoke void %body(ptr %ctx) to label %ok unwind label %dispatch
ok:
  ret i32 0
dispatch:
  %cs = catchswitch within none [label %pad] unwind to caller
pad:
  %cp = catchpad within %cs [ptr null]
  %object = call ptr @llvm.wasm.get.exception(token %cp)
  %ours = call i1 @js_wasm_eh_is_perry(ptr %object) [ "funclet"(token %cp) ]
  br i1 %ours, label %accepted, label %foreign
accepted:
  call void @js_wasm_eh_pad_enter(ptr %checkpoint) [ "funclet"(token %cp) ]
  catchret from %cp to label %caught
foreign:
  call void @llvm.wasm.rethrow() [ "funclet"(token %cp) ]
  unreachable
caught:
  ret i32 1
}
