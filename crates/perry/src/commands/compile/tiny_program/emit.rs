//! Minimal native entry and output. The only external runtime dependency is libc.
//! No managed allocation, root registration, JS callbacks, or task pump exists.
use std::fmt::Write;

use super::analysis::ProvenProgram;

pub(super) fn llvm_ir(program: &ProvenProgram) -> String {
    let mut ir = String::from("; Perry proven constant-string program: no managed runtime\n");
    for (index, output) in program.outputs().iter().enumerate() {
        let bytes: String = output.bytes.iter().map(|b| format!("\\{b:02X}")).collect();
        writeln!(ir, "@perry_tiny_text_{index} = private unnamed_addr constant [{} x i8] c\"{bytes}\", align 1", output.bytes.len()).unwrap();
    }
    if !program.outputs().is_empty() {
        let (errno, again, nfds) = if cfg!(target_os = "macos") {
            ("__error", 35, "i32")
        } else {
            ("__errno_location", 11, "i64")
        };
        ir.push_str(
            &OUTPUT_HELPER
                .replace("ERRNO_FN", errno)
                .replace("EAGAIN_VALUE", &again.to_string())
                .replace("NFDS_TYPE", nfds),
        );
    }
    ir.push_str("define i32 @main(i32 %argc, ptr %argv) {\nentry:\n");
    if !program.outputs().is_empty() {
        // Node ignores SIGPIPE and console's default ignoreErrors suppresses
        // write failures. Keep later stderr calls alive after stdout closes.
        ir.push_str("  %old_pipe = call ptr @signal(i32 13, ptr inttoptr (i64 1 to ptr))\n");
    }
    for (index, output) in program.outputs().iter().enumerate() {
        writeln!(
            ir,
            "  call void @perry_tiny_write(i32 {}, ptr @perry_tiny_text_{index}, i64 {})",
            output.fd,
            output.bytes.len()
        )
        .unwrap();
    }
    ir.push_str("  ret i32 0\n}\n");
    ir
}

// write() may be partial or interrupted. A nonblocking inherited pipe/socket
// can return EAGAIN: wait for writability and retry, without an event loop or
// background worker. Permanent errors close only that console stream. Source
// eligibility excludes handlers, stream mutation, and every other observer.
const OUTPUT_HELPER: &str = r#"
@perry_tiny_closed = internal global [3 x i1] zeroinitializer

declare i64 @write(i32, ptr, i64)
declare ptr @ERRNO_FN()
declare i32 @poll(ptr, NFDS_TYPE, i32)
declare ptr @signal(i32, ptr)

define internal void @perry_tiny_write(i32 %fd, ptr %bytes, i64 %length) {
entry:
  %slot = getelementptr [3 x i1], ptr @perry_tiny_closed, i32 0, i32 %fd
  %closed = load i1, ptr %slot
  %pollfd = alloca { i32, i16, i16 }, align 4
  %poll_events = getelementptr { i32, i16, i16 }, ptr %pollfd, i32 0, i32 1
  %poll_revents = getelementptr { i32, i16, i16 }, ptr %pollfd, i32 0, i32 2
  store i32 %fd, ptr %pollfd
  store i16 4, ptr %poll_events
  br i1 %closed, label %done, label %loop
loop:
  %offset = phi i64 [ 0, %entry ], [ %next, %written ], [ %offset, %retry ], [ %offset, %wait_done ]
  %data = getelementptr i8, ptr %bytes, i64 %offset
  %left = sub i64 %length, %offset
  %count = call i64 @write(i32 %fd, ptr %data, i64 %left)
  %positive = icmp sgt i64 %count, 0
  br i1 %positive, label %written, label %failed
written:
  %next = add i64 %offset, %count
  %complete = icmp eq i64 %next, %length
  br i1 %complete, label %done, label %loop
failed:
  %negative = icmp slt i64 %count, 0
  br i1 %negative, label %retry, label %close
retry:
  %errno_ptr = call ptr @ERRNO_FN()
  %errno = load i32, ptr %errno_ptr
  %interrupted = icmp eq i32 %errno, 4
  br i1 %interrupted, label %loop, label %check_wait
check_wait:
  %again = icmp eq i32 %errno, EAGAIN_VALUE
  br i1 %again, label %wait, label %close
wait:
  store i16 0, ptr %poll_revents
  %ready = call i32 @poll(ptr %pollfd, NFDS_TYPE 1, i32 -1)
  %poll_failed = icmp slt i32 %ready, 0
  br i1 %poll_failed, label %wait_error, label %wait_done
wait_error:
  %wait_errno_ptr = call ptr @ERRNO_FN()
  %wait_errno = load i32, ptr %wait_errno_ptr
  %wait_interrupted = icmp eq i32 %wait_errno, 4
  br i1 %wait_interrupted, label %wait, label %close
wait_done:
  br label %loop
close:
  store i1 true, ptr %slot
  br label %done
done:
  ret void
}
"#;

#[cfg(test)]
mod tests {
    use super::super::analysis::analyze;
    use super::*;

    #[test]
    fn empty_entry_has_no_external_dependencies() {
        let ir = llvm_ir(&analyze("", "app.ts").unwrap());
        assert!(!ir.contains("declare "));
        assert!(!ir.contains("call "));
        assert!(ir.contains("ret i32 0"));
    }

    #[test]
    fn output_dependencies_are_explicit_and_do_not_include_perry_runtime() {
        let ir = llvm_ir(&analyze("console.log('a\\0%'); console.error('b');", "app.ts").unwrap());
        let declarations: Vec<_> = ir
            .lines()
            .filter(|line| line.starts_with("declare "))
            .collect();
        assert_eq!(declarations.len(), 4);
        for symbol in ["@write(", "@poll(", "@signal("] {
            assert!(declarations.iter().any(|line| line.contains(symbol)));
        }
        assert!(ir.contains(r#"c"\61\00\25\0A""#));
        for excluded in [
            "@js_",
            "@perry_gc",
            "mimalloc",
            "@malloc",
            "@calloc",
            "promise",
            "shadow",
            "statepoint",
        ] {
            assert!(!ir.contains(excluded), "unexpected dependency {excluded}");
        }
    }
}
