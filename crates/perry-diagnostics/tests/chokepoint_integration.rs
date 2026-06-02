//! End-to-end check that the emitter chokepoint forwards the right
//! diagnostics into the compat-report queue.
//!
//! Validates the privacy boundary: when a sink is installed, a diagnostic
//! whose `DiagnosticCode` is in the reportable set produces a queued
//! `PendingReport` whose `raw_snippet` matches what was in the source
//! file at the diagnostic's span. Diagnostics with non-reportable codes
//! never enqueue anything.

use perry_diagnostics::compat_report::{set_report_sink, PendingReport, ReportSink};
use perry_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticEmitter, SimpleEmitter, SourceCache, Span,
};
use std::sync::{Arc, Mutex, OnceLock};

/// All three tests share the global sink slot via `set_report_sink`, so
/// they MUST run serially.
fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
struct CapturingSink {
    captured: Arc<Mutex<Vec<PendingReport>>>,
}

impl ReportSink for CapturingSink {
    fn submit(&self, pending: PendingReport) {
        self.captured.lock().unwrap().push(pending);
    }
}

#[test]
fn reportable_diagnostic_enqueues_pending_report() {
    let _g = test_lock();
    let captured = Arc::new(Mutex::new(Vec::<PendingReport>::new()));
    set_report_sink(Box::new(CapturingSink {
        captured: captured.clone(),
    }));

    let mut cache = SourceCache::new();
    let src = "let x = await foo();\n";
    let file_id = cache.add_file("smoke.ts", src.to_string());

    // Build a UnsupportedExpression diagnostic whose span covers `await foo()`
    let start = src.find("await").unwrap() as u32;
    let end = (src.find(";").unwrap()) as u32;
    let diag = Diagnostic::error(DiagnosticCode::UnsupportedExpression, "test")
        .with_span(Span::new(file_id, start, end))
        .build();

    let mut buf = Vec::new();
    let mut emitter = SimpleEmitter::new(&mut buf);
    emitter.emit(&diag, &cache).unwrap();

    let captured = captured.lock().unwrap();
    assert_eq!(captured.len(), 1, "expected exactly one queued report");
    let report = &captured[0];
    assert_eq!(report.code, DiagnosticCode::UnsupportedExpression);
    // The raw snippet pulled from the cache must match the source bytes
    // at the diagnostic's span.
    assert_eq!(report.raw_snippet, "await foo()");
}

#[test]
fn non_reportable_diagnostic_does_not_enqueue() {
    let _g = test_lock();
    let captured = Arc::new(Mutex::new(Vec::<PendingReport>::new()));
    set_report_sink(Box::new(CapturingSink {
        captured: captured.clone(),
    }));

    let mut cache = SourceCache::new();
    let file_id = cache.add_file("x.ts", "const x = 1;".to_string());
    let diag = Diagnostic::error(DiagnosticCode::ParseError, "parse failure")
        .with_span(Span::new(file_id, 0, 4))
        .build();

    let mut buf = Vec::new();
    let mut emitter = SimpleEmitter::new(&mut buf);
    emitter.emit(&diag, &cache).unwrap();

    let captured = captured.lock().unwrap();
    assert!(
        captured.is_empty(),
        "ParseError is not in the reportable set; got: {:?}",
        *captured
    );
}

#[test]
fn dummy_span_diagnostic_does_not_enqueue() {
    let _g = test_lock();
    let captured = Arc::new(Mutex::new(Vec::<PendingReport>::new()));
    set_report_sink(Box::new(CapturingSink {
        captured: captured.clone(),
    }));

    let mut cache = SourceCache::new();
    let _id = cache.add_file("x.ts", "const x = 1;".to_string());
    // Diagnostic with no span — no snippet to redact, must not enqueue.
    let diag = Diagnostic::error(DiagnosticCode::UnsupportedExpression, "no span").build();

    let mut buf = Vec::new();
    let mut emitter = SimpleEmitter::new(&mut buf);
    emitter.emit(&diag, &cache).unwrap();

    assert!(captured.lock().unwrap().is_empty());
}
