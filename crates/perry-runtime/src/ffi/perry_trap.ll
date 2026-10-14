; Runtime catch by landing pad (ehfree slice 2b).
;
; A Rust-side JS catch (`exception::trap`, behind `catch_js_throw`,
; `js_call_catching`, the runtime drains and `js_trap_call`) runs its body
; through this shim. The body is invoked under a catch-all landing pad with
; Perry's personality: exactly the shape of a generated `try`
; (perry-codegen stmt/try_stmt.rs), so `js_throw` reaches it with the same
; transport (`_Unwind_RaiseException`, the aarch64 owned walker, or
; `RtlUnwindEx` on windows-msvc). No frame is ever returned into twice, so
; no setjmp contract is needed and the shim needs no C compiler.
;
; The personality finds whichever handler is innermost, shim or generated
; pad. The thrown value stays in the runtime's rooted TLS slot; the
; `{ptr, i32}` pair is ignored. Like every Perry pad, the pad's first call is
; `js_eh_pad_enter(llvm.stacksave(), %hh)`: it pops the managed-stack entries
; of the frames the throw abandoned (those marked below this frame's stack
; pointer; `exception::marks`) and truncates the runtime-handle stack to %hh,
; its height when the shim was entered (`js_eh_handle_height`).
;
; Returns 0 when `body` returned, 1 when a JS throw landed in the pad.
;
; `perry_trap_call_base` is the worker base trap (`run_worker_body`). It is a
; separate function so the personality can single it out while
; `hand_to_worker_base` carries an uncaught exception past every other
; handler; its pad returns 2 so identical-code folding can never merge the two.
;
; Built once with the runtime (build.rs, `clang -x ir`), never per program.
; No triple or datalayout here: the build passes the target.

declare i32 @perry_eh_personality(...)
declare void @js_eh_pad_enter(ptr, i64) nounwind
declare i64 @js_eh_handle_height() nounwind
declare ptr @llvm.stacksave.p0()

define i32 @perry_trap_call(ptr %body, ptr %ctx) #0 personality ptr @perry_eh_personality {
entry:
  %hh = call i64 @js_eh_handle_height()
  invoke void %body(ptr %ctx)
          to label %ok unwind label %pad

ok:
  ret i32 0

pad:
  %lp = landingpad { ptr, i32 }
          catch ptr null
  %sp = call ptr @llvm.stacksave.p0()
  call void @js_eh_pad_enter(ptr %sp, i64 %hh)
  ret i32 1
}

define i32 @perry_trap_call_base(ptr %body, ptr %ctx) #0 personality ptr @perry_eh_personality {
entry:
  %hh = call i64 @js_eh_handle_height()
  invoke void %body(ptr %ctx)
          to label %ok unwind label %pad

ok:
  ret i32 0

pad:
  %lp = landingpad { ptr, i32 }
          catch ptr null
  %sp = call ptr @llvm.stacksave.p0()
  call void @js_eh_pad_enter(ptr %sp, i64 %hh)
  ret i32 2
}

; Frame pointers keep the rbp/x29 chain whole for Error.stack and the
; collector's frame walk; uwtable gives the shim its FDE / compact-unwind
; entry (and .pdata/.xdata on windows-msvc) with the personality and LSDA.
attributes #0 = { noinline uwtable "frame-pointer"="all" }
