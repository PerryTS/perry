//! Native clocks and Turnloop-backed WASI deadline resources.
mod datetime;
#[cfg(feature = "p3")]
mod p3;
mod system;
pub(crate) use datetime::{Datetime, DatetimeError};

use crate::io::poll::{subscribe, DynPollable, Pollable};
use crate::p2::bindings::clocks::{monotonic_clock, wall_clock};
use perry_component_io::{Request, Submitter};
use std::time::{Duration, Instant, SystemTime};
use system::{HostMonotonicClock, HostWallClock};
use wasmtime::component::{HasData, Resource, ResourceTable};

pub struct WasiClocks;
impl HasData for WasiClocks {
    type Data<'a> = WasiClocksCtxView<'a>;
}

pub struct WasiClocksCtx {
    monotonic: Box<dyn HostMonotonicClock + Send>,
    wall: Box<dyn HostWallClock + Send>,
    submitter: Submitter,
}
impl WasiClocksCtx {
    pub fn new(submitter: Submitter) -> Self {
        Self {
            monotonic: system::monotonic_clock(),
            wall: system::wall_clock(),
            submitter,
        }
    }
}

pub struct WasiClocksCtxView<'a> {
    pub ctx: &'a mut WasiClocksCtx,
    pub table: &'a mut ResourceTable,
}

impl wall_clock::Host for WasiClocksCtxView<'_> {
    fn now(&mut self) -> wasmtime::Result<wall_clock::Datetime> {
        // Negative timestamps cannot be represented by the P2 unsigned ABI.
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        Ok(wall_clock::Datetime {
            seconds: now.as_secs(),
            nanoseconds: now.subsec_nanos(),
        })
    }
    fn resolution(&mut self) -> wasmtime::Result<wall_clock::Datetime> {
        let resolution = self.ctx.wall.resolution();
        Ok(wall_clock::Datetime {
            seconds: resolution.as_secs(),
            nanoseconds: resolution.subsec_nanos(),
        })
    }
}

impl monotonic_clock::Host for WasiClocksCtxView<'_> {
    fn now(&mut self) -> wasmtime::Result<u64> {
        Ok(self.ctx.monotonic.now())
    }
    fn resolution(&mut self) -> wasmtime::Result<u64> {
        Ok(self.ctx.monotonic.resolution())
    }
    fn subscribe_instant(&mut self, when: u64) -> wasmtime::Result<Resource<DynPollable>> {
        self.subscribe(Duration::from_nanos(
            when.saturating_sub(self.ctx.monotonic.now()),
        ))
    }
    fn subscribe_duration(&mut self, duration: u64) -> wasmtime::Result<Resource<DynPollable>> {
        self.subscribe(Duration::from_nanos(duration))
    }
}

impl WasiClocksCtxView<'_> {
    fn subscribe(&mut self, duration: Duration) -> wasmtime::Result<Resource<DynPollable>> {
        // Anchor the deadline at the guest call, not the driver's eventual first
        // poll. Delayed/queued work must not restart a relative timeout.
        let deadline = if duration.is_zero() {
            Deadline::Ready
        } else if let Some(when) = Instant::now().checked_add(duration) {
            Deadline::Pending(
                self.ctx
                    .submitter
                    .submit(move |io| async move { io.sleep_until(when).await })?,
            )
        } else {
            Deadline::Never
        };
        let resource = self.table.push(deadline)?;
        subscribe(self.table, resource)
    }
}

enum Deadline {
    Pending(Request<Result<(), turnloop::Error>>),
    Ready,
    Failed(String),
    Never,
}

#[async_trait::async_trait]
impl Pollable for Deadline {
    async fn ready(&mut self) -> wasmtime::Result<()> {
        if let Self::Pending(request) = self {
            *self = match request.await {
                Ok(Ok(())) => Self::Ready,
                Ok(Err(error)) => Self::Failed(error.to_string()),
                Err(error) => Self::Failed(error.to_string()),
            };
        }
        match self {
            Self::Ready => Ok(()),
            Self::Failed(error) => wasmtime::bail!("WASI clock driver failed: {error}"),
            Self::Never => std::future::pending().await,
            Self::Pending(_) => unreachable!("completed clock task remains pending"),
        }
    }
}

impl TryFrom<Datetime> for wall_clock::Datetime {
    type Error = DatetimeError;
    fn try_from(value: Datetime) -> Result<Self, Self::Error> {
        Ok(Self {
            seconds: value.seconds.try_into()?,
            nanoseconds: value.nanoseconds,
        })
    }
}
