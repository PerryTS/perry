//! Keep guest diagnostics nonblocking, including module initialization and
//! synchronous exports. Flush only after the guest Store has retired.
use perry_component_host::WasiOptions;
use perry_wasi::pipe::MemoryOutputPipe;
use std::io::Write;

pub struct GuestDiagnostics(MemoryOutputPipe);

impl Drop for GuestDiagnostics {
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.0.contents());
    }
}

pub fn configure(wasi: &mut WasiOptions, moving: bool, seed: u64) -> Option<GuestDiagnostics> {
    if !moving {
        return None;
    }
    let output = MemoryOutputPipe::new(16 * 1024 * 1024);
    wasi.inherit_stderr = false;
    wasi.stderr_buffer = Some(output.clone());
    for name in [
        "PERRY_GC_FORCE_EVACUATE",
        "PERRY_GC_VERIFY_EVACUATION",
        "PERRY_GC_DIAG",
    ] {
        wasi.env.insert(name.into(), "1".into());
    }
    // This instrument-specific knob must reject the plain release archive.
    // Basic evacuation/verification knobs alone do not prove that distinction.
    wasi.env
        .insert("PERRY_GC_SCHEDULE_SEED".into(), seed.to_string());
    wasi.env.insert("PERRY_GC_SCHEDULE_RATE".into(), "1".into());
    Some(GuestDiagnostics(output))
}
