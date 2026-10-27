use super::*;
use crate::gc::RuntimeHandle;

pub(super) struct Batch<'a> {
    component: RuntimeHandle<'a>,
    frames: callback::FrameBatch<'a>,
    cleanup: bool,
    first_exception: std::cell::Cell<Option<u64>>,
}
impl<'a> Batch<'a> {
    pub fn new(scope: &'a RuntimeHandleScope, component: f64, cleanup: bool) -> Self {
        let component = scope.root_nanbox_f64(component);
        Self {
            component,
            frames: callback::FrameBatch::new(scope, component.get_nanbox_f64()),
            cleanup,
            first_exception: std::cell::Cell::new(None),
        }
    }
    pub fn token(&self) -> u64 {
        self as *const Self as usize as u64
    }
    pub fn frame(&self, origin: u64) -> &callback::InvocationFrame<'a> {
        let component = self.component.get_nanbox_f64();
        let execution = ids(component, origin);
        let saved = records::get(origin_row(component, origin), records::CONTEXT);
        self.frames.for_context(origin, saved, execution)
    }
    pub fn callback_error(&self) -> Option<f64> {
        self.first_exception.get().map(|origin| {
            let frame = self.frame(origin);
            if frame.threw.get() {
                frame.error.get_nanbox_f64()
            } else {
                records::get(
                    origin_row(self.component.get_nanbox_f64(), origin),
                    records::ERROR,
                )
            }
        })
    }
    pub fn rethrow_callback_error(&self) {
        if let Some(error) = self.callback_error() {
            crate::exception::js_throw(error);
        }
    }
    pub fn cleanup(&self, body: impl FnOnce() -> f64) -> Result<f64, f64> {
        self.frame(0).run_cleanup(body)
    }
    fn dispatch_callback(
        &self,
        origin: u64,
        body: impl FnOnce(&callback::Frame<'_>) -> i32,
        out: *mut Buffer,
    ) -> i32 {
        let mut status = ERROR;
        let mut replied = false;
        let outcome = crate::exception::catch_js_throw(|| {
            let frame = self.frame(origin);
            let execute = || {
                status = body(frame);
                replied = true;
                undefined()
            };
            let result = if self.cleanup {
                frame.run_cleanup(execute)
            } else {
                frame.run(execute)
            };
            if let Err(error) = result {
                frame.capture(error);
            }
            if frame.threw.get() {
                if self.first_exception.get().is_none() {
                    self.first_exception.set(Some(origin));
                }
                remember(
                    self.component.get_nanbox_f64(),
                    origin,
                    frame.error.get_nanbox_f64(),
                );
                if replied {
                    unsafe {
                        callback::free(*out);
                    }
                }
                callback::reply(out, b"JavaScript component callback threw".to_vec());
                status = ERROR;
            }
        });
        if let Err(error) = outcome {
            if self.first_exception.get().is_none() {
                self.first_exception.set(Some(0));
            }
            if replied {
                unsafe {
                    callback::free(*out);
                }
            }
            remember(self.component.get_nanbox_f64(), 0, error);
            callback::reply(out, b"Invalid persistent callback context".to_vec());
            ERROR
        } else {
            status
        }
    }
}
pub(super) unsafe extern "C" fn invoke(
    token: u64,
    origin: u64,
    interface: *const u8,
    interface_len: usize,
    function: *const u8,
    function_len: usize,
    args: *const u8,
    args_len: usize,
    out: *mut Buffer,
) -> i32 {
    let batch = &*(token as usize as *const Batch<'_>);
    batch.dispatch_callback(
        origin,
        |frame| {
            callback::invoke_for(
                frame,
                Some(origin),
                interface,
                interface_len,
                function,
                function_len,
                args,
                args_len,
                out,
            )
        },
        out,
    )
}
pub(super) unsafe extern "C" fn resource_event(
    token: u64,
    origin: u64,
    event: *const u8,
    event_len: usize,
    out: *mut Buffer,
) -> i32 {
    let batch = &*(token as usize as *const Batch<'_>);
    batch.dispatch_callback(
        origin,
        |frame| callback::resource_for(frame, Some(origin), event, event_len, out),
        out,
    )
}
