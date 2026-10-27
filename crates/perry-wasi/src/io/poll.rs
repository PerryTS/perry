// Adapted from Wasmtime 48.0.5 (Bytecode Alliance contributors).
// SPDX-License-Identifier: Apache-2.0 WITH LLVM-exception
// See PROVENANCE.md and LICENSE.

use std::{any::Any, future::Future, pin::Pin};
use wasmtime::{
    component::{Resource, ResourceTable},
    Result,
};

pub type DynFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
pub type MakeFuture = for<'a> fn(&'a mut dyn Any) -> DynFuture<'a>;

/// The host representation of the `wasi:io/poll.pollable` resource.
///
/// A pollable is not the same thing as a Rust Future: the same pollable may be used to
/// repeatedly check for readiness of a given condition, e.g. if a stream is readable
/// or writable. So, rather than containing a Future, which can only become Ready once, a
/// `DynPollable` contains a way to create a Future in each call to `poll`.
pub struct DynPollable {
    pub(crate) index: u32,
    pub(crate) make_future: MakeFuture,
    pub(crate) remove_index_on_delete: Option<fn(&mut ResourceTable, u32) -> Result<()>>,
}

#[async_trait::async_trait]
pub trait Pollable: Send + 'static {
    /// An asynchronous function which resolves when this object's readiness
    /// operation is ready.
    ///
    /// This function is invoked as part of `poll` in `wasi:io/poll`. The
    /// meaning of when this function Returns depends on what object this
    /// [`Pollable`] is attached to. When the returned future resolves then the
    /// corresponding call to `wasi:io/poll` will return.
    ///
    /// Ordinary stream errors remain on their read/write accessors. A failure
    /// of the native readiness mechanism is a host trap, never false readiness.
    async fn ready(&mut self) -> Result<()>;
}

/// Creates a `wasi:io/poll.pollable` resource which is subscribed to the provided
/// `resource`.
///
/// If `resource` is an owned resource then it will be deleted when the returned
/// resource is deleted. Otherwise the returned resource is considered a "child"
/// of the given `resource` which means that the given resource cannot be
/// deleted while the `pollable` is still alive.
pub fn subscribe<T>(
    table: &mut ResourceTable,
    resource: Resource<T>,
) -> Result<Resource<DynPollable>>
where
    T: Pollable,
{
    fn make_future<'a, T>(stream: &'a mut dyn Any) -> DynFuture<'a>
    where
        T: Pollable,
    {
        stream.downcast_mut::<T>().unwrap().ready()
    }

    let pollable = DynPollable {
        index: resource.rep(),
        remove_index_on_delete: if resource.owned() {
            Some(|table, idx| {
                let resource = Resource::<T>::new_own(idx);
                table.delete(resource)?;
                Ok(())
            })
        } else {
            None
        },
        make_future: make_future::<T>,
    };

    match table.push_child(pollable, &resource) {
        Ok(pollable) => Ok(pollable),
        Err(error) => {
            if resource.owned() {
                // A failed subscription must not leak an internal timer/task.
                let _ = table.delete(resource);
            }
            Err(error.into())
        }
    }
}

// Streams report typed errors through their read/write operations. Readiness
// itself is infallible for these upstream stream traits.
#[async_trait::async_trait]
impl Pollable for wasmtime_wasi_io::streams::DynInputStream {
    async fn ready(&mut self) -> Result<()> {
        wasmtime_wasi_io::poll::Pollable::ready(self).await;
        Ok(())
    }
}
#[async_trait::async_trait]
impl Pollable for wasmtime_wasi_io::streams::DynOutputStream {
    async fn ready(&mut self) -> Result<()> {
        wasmtime_wasi_io::poll::Pollable::ready(self).await;
        Ok(())
    }
}
