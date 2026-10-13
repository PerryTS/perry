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
; `{ptr, i32}` pair is ignored. The runtime handler stack is unchanged by
; this file: the Rust caller pushes the handler entry (its savepoint is
; restored at throw time, in `js_throw`) and pops it after the shim returns.
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

define i32 @perry_trap_call(ptr %body, ptr %ctx) #0 personality ptr @perry_eh_personality {
entry:
  invoke void %body(ptr %ctx)
          to label %ok unwind label %pad

ok:
  ret i32 0

pad:
  %lp = landingpad { ptr, i32 }
          catch ptr null
  ret i32 1
}

define i32 @perry_trap_call_base(ptr %body, ptr %ctx) #0 personality ptr @perry_eh_personality {
entry:
  invoke void %body(ptr %ctx)
          to label %ok unwind label %pad

ok:
  ret i32 0

pad:
  %lp = landingpad { ptr, i32 }
          catch ptr null
  ret i32 2
}

; Frame pointers keep the rbp/x29 chain whole for Error.stack and the
; collector's frame walk; uwtable gives the shim its FDE / compact-unwind
; entry (and .pdata/.xdata on windows-msvc) with the personality and LSDA.
attributes #0 = { noinline uwtable "frame-pointer"="all" }
