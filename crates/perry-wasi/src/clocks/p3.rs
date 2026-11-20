use super::{Datetime, WasiClocks, WasiClocksCtxView};
use crate::p3::bindings::clocks::{monotonic_clock, system_clock, types};
use perry_component_io::{Request, Submitter};
use std::time::{Duration, Instant, SystemTime};
use wasmtime::component::Accessor;

impl types::Host for WasiClocksCtxView<'_> {}

impl system_clock::Host for WasiClocksCtxView<'_> {
    fn now(&mut self) -> wasmtime::Result<system_clock::Instant> {
        let now = Datetime::try_from(SystemTime::now())?;
        Ok(system_clock::Instant {
            seconds: now.seconds,
            nanoseconds: now.nanoseconds,
        })
    }

    fn get_resolution(&mut self) -> wasmtime::Result<types::Duration> {
        Ok(self.ctx.wall.resolution().as_nanos().try_into()?)
    }
}

impl monotonic_clock::Host for WasiClocksCtxView<'_> {
    fn now(&mut self) -> wasmtime::Result<monotonic_clock::Mark> {
        Ok(self.ctx.monotonic.now())
    }

    fn get_resolution(&mut self) -> wasmtime::Result<types::Duration> {
        Ok(self.ctx.monotonic.resolution())
    }
}

// Own native work only. Store borrows end before the await; dropping the
// enclosing async import drops its Request and cancels the driver's timer.
enum Wait {
    Ready,
    Pending(Request<Result<(), turnloop::Error>>),
    Never,
}

impl Wait {
    fn new(submitter: &Submitter, nanos: u64) -> wasmtime::Result<Self> {
        if nanos == 0 {
            return Ok(Self::Ready);
        }
        // Anchor relative waits now, before the driver's first poll.
        let Some(deadline) = Instant::now().checked_add(Duration::from_nanos(nanos)) else {
            return Ok(Self::Never);
        };
        Ok(Self::Pending(submitter.submit(move |io| async move {
            io.sleep_until(deadline).await
        })?))
    }

    async fn complete(self) -> wasmtime::Result<()> {
        match self {
            Self::Ready => Ok(()),
            Self::Pending(request) => {
                request.await??;
                Ok(())
            }
            Self::Never => std::future::pending().await,
        }
    }
}

impl<T> monotonic_clock::HostWithStore<T> for WasiClocks {
    async fn wait_until(
        store: &Accessor<T, Self>,
        when: monotonic_clock::Mark,
    ) -> wasmtime::Result<()> {
        let wait = store.with(|mut access| {
            let ctx = access.get().ctx;
            Wait::new(&ctx.submitter, when.saturating_sub(ctx.monotonic.now()))
        })?;
        wait.complete().await
    }

    async fn wait_for(
        store: &Accessor<T, Self>,
        how_long: types::Duration,
    ) -> wasmtime::Result<()> {
        let wait = store.with(|mut access| Wait::new(&access.get().ctx.submitter, how_long))?;
        wait.complete().await
    }
}
