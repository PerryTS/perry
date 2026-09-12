//! Source display streams original UTF-16 units into final GC storage. Only
//! integer escape/class state survives a poll; no native pattern copy exists.
use super::perex_api::{OUTPUT_BYTES, QUANTUM, WORK};
use super::perex_owner::HeapSubject;
use super::perex_runtime::{self as host, EngineError};
use super::RegExpHeader;
use crate::gc::RuntimeHandleScope;
use crate::string::StringHeader;
use perex::binding::BoundSubject;
use perex::span::{BoundSpan, ReadError, ReadProgress, Span};
use perex::Budget;

#[derive(Default)]
struct Escape {
    escaped: bool,
    class: bool,
}

impl Escape {
    fn unit(
        &mut self,
        unit: u16,
        out: &mut dyn FnMut(u16) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let line: Option<&[u8]> = match unit {
            10 => Some(b"\\n"),
            13 => Some(b"\\r"),
            0x2028 => Some(b"\\u2028"),
            0x2029 => Some(b"\\u2029"),
            _ => None,
        };
        if let Some(line) = line {
            self.escaped = false;
            for &byte in line {
                out(u16::from(byte))?;
            }
        } else if self.escaped {
            self.escaped = false;
            out(92)?;
            out(unit)?;
        } else {
            match unit {
                92 => self.escaped = true,
                91 => {
                    self.class = true;
                    out(unit)?;
                }
                93 => {
                    self.class = false;
                    out(unit)?;
                }
                47 if !self.class => {
                    out(92)?;
                    out(unit)?;
                }
                _ => out(unit)?,
            }
        }
        Ok(())
    }
}

fn error(e: ReadError<super::perex_owner::OwnerError, EngineError>) -> EngineError {
    match e {
        ReadError::Consumer(e) => e,
        ReadError::Subject(e) => EngineError::Subject(e),
        ReadError::WorkLimit => EngineError::Execution(perex::executor::ExecError::WorkLimit),
        _ => EngineError::InvalidSpan,
    }
}

pub(super) fn source(re: *const RegExpHeader) -> Result<*mut StringHeader, EngineError> {
    render(re, false)
}
pub(super) fn to_string(re: *const RegExpHeader) -> Result<*mut StringHeader, EngineError> {
    render(re, true)
}

fn render(re: *const RegExpHeader, delimited: bool) -> Result<*mut StringHeader, EngineError> {
    if !super::is_valid_regex_ptr(re) {
        return Ok(super::js_string_from_str(if delimited {
            "/(?:)/"
        } else {
            "(?:)"
        }));
    }
    let scope = RuntimeHandleScope::new();
    let (source, flags) = unsafe { ((*re).pattern_ptr, (*re).flags_ptr) };
    let source = scope.root_string_ptr(source);
    let flags = scope.root_string_ptr(flags);
    let source_len =
        source.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
    let flags_len = flags.with_const_ptr::<StringHeader, _>(|s| unsafe { (*s).utf16_len as usize });
    let bind = |root| {
        BoundSubject::new(
            unsafe { HeapSubject::new(root) }
                .map_err(|e| EngineError::Subject(perex::binding::SubjectError::Resource(e)))?,
        )
        .map_err(|e| EngineError::Subject(e.error))
    };
    let source = bind(source)?;
    let flags = bind(flags)?;
    let mut sources = [
        BoundSpan::new(
            &source,
            Span::new(0, source_len).ok_or(EngineError::InvalidSpan)?,
        )
        .map_err(|_| EngineError::InvalidSpan)?,
        BoundSpan::new(
            &source,
            Span::new(0, source_len).ok_or(EngineError::InvalidSpan)?,
        )
        .map_err(|_| EngineError::InvalidSpan)?,
    ];
    let mut flag_readers = [
        BoundSpan::new(
            &flags,
            Span::new(0, flags_len).ok_or(EngineError::InvalidSpan)?,
        )
        .map_err(|_| EngineError::InvalidSpan)?,
        BoundSpan::new(
            &flags,
            Span::new(0, flags_len).ok_or(EngineError::InvalidSpan)?,
        )
        .map_err(|_| EngineError::InvalidSpan)?,
    ];
    let mut states = [Escape::default(), Escape::default()];
    let mut phase = [0; 2];
    super::perex_strings::copy_units(
        None,
        &mut Budget::new(WORK),
        OUTPUT_BYTES,
        QUANTUM,
        &mut host::poll,
        |pass, quantum, budget, out| {
            if phase[pass] == 0 {
                if delimited {
                    host::charge(budget, 1)?;
                    out(47)?;
                }
                phase[pass] = 1;
            }
            if phase[pass] == 1 {
                let progress = sources[pass]
                    .try_fold(quantum, budget, |unit| states[pass].unit(unit, out))
                    .map_err(error)?;
                if progress == ReadProgress::Pending {
                    return Ok(progress);
                }
                if source_len == 0 {
                    host::charge(budget, 4)?;
                    for &byte in b"(?:)" {
                        out(u16::from(byte))?;
                    }
                }
                if states[pass].escaped {
                    out(92)?;
                }
                phase[pass] = 2;
                if delimited {
                    host::charge(budget, 1)?;
                    out(47)?;
                    return Ok(ReadProgress::Pending);
                }
            }
            if delimited {
                flag_readers[pass]
                    .try_fold(quantum, budget, out)
                    .map_err(error)
            } else {
                Ok(ReadProgress::Complete)
            }
        },
    )
}
