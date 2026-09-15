//! Balanced native handle references shared with extension crates.
pub(crate) use perry_ffi::activity::Reference;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn reference_balances_close_error_cancel_and_repeated_unref() {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        assert_eq!(COUNT.load(Ordering::Acquire), 0);
        let mut handle = Reference::new(&COUNT, true);
        assert_eq!(COUNT.load(Ordering::Acquire), 1);
        handle.set(false);
        handle.set(false);
        assert_eq!(COUNT.load(Ordering::Acquire), 0);
        handle.set(true);
        drop(handle);
        assert_eq!(COUNT.load(Ordering::Acquire), 0);
        let error: Result<(), ()> = (|| {
            let _handle = Reference::new(&COUNT, true);
            assert_eq!(COUNT.load(Ordering::Acquire), 1);
            Err(())
        })();
        assert!(error.is_err());
        let mut cancelled = vec![Reference::new(&COUNT, true)];
        assert_eq!(COUNT.load(Ordering::Acquire), 1);
        cancelled.clear();
        assert_eq!(COUNT.load(Ordering::Acquire), 0);
    }
}
