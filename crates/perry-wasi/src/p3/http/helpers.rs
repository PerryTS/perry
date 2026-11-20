//! Canonical future callbacks run only while Wasmtime grants Store access.
use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use wasmtime::component::{FutureConsumer, FutureProducer, FutureReader, Lift, Lower, Source};
use wasmtime::{AsContextMut, StoreContextMut};

type Callback<D, T, U> = Box<dyn FnOnce(&mut D, T) -> wasmtime::Result<U> + Send>;

pub fn pipe<D: 'static, T: Lift + 'static>(
    store: impl AsContextMut<Data = D>,
    future: FutureReader<T>,
    callback: impl FnOnce(&mut D, T) -> wasmtime::Result<()> + Send + 'static,
) -> wasmtime::Result<()> {
    struct Consumer<D, T> {
        callback: Option<Callback<D, T, ()>>,
    }
    impl<D: 'static, T: Lift + 'static> FutureConsumer<D> for Consumer<D, T> {
        type Item = T;
        fn poll_consume(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            mut store: StoreContextMut<D>,
            mut source: Source<T>,
            _: bool,
        ) -> Poll<wasmtime::Result<()>> {
            let mut value = None;
            source.read(&mut store, &mut value)?;
            let value = value.ok_or_else(|| wasmtime::format_err!("HTTP future has no value"))?;
            let callback = self
                .callback
                .take()
                .ok_or_else(|| wasmtime::format_err!("HTTP future consumed twice"))?;
            Poll::Ready(callback(store.data_mut(), value))
        }
    }
    future.pipe(
        store,
        Consumer {
            callback: Some(Box::new(callback)),
        },
    )
}

pub fn future<D: 'static, T: Lift + Lower + 'static, U: 'static>(
    store: impl AsContextMut<Data = D>,
    future: impl Future<Output = wasmtime::Result<U>> + Send + 'static,
    callback: impl FnOnce(&mut D, U) -> wasmtime::Result<T> + Send + 'static,
) -> wasmtime::Result<FutureReader<T>> {
    struct Producer<D, T, U> {
        pending: Pin<Box<dyn Future<Output = wasmtime::Result<U>> + Send>>,
        callback: Option<Callback<D, U, T>>,
        marker: PhantomData<fn(D, T)>,
    }
    impl<D: 'static, T: 'static, U: 'static> FutureProducer<D> for Producer<D, T, U> {
        type Item = T;
        fn poll_produce(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            mut store: StoreContextMut<D>,
            finish: bool,
        ) -> Poll<wasmtime::Result<Option<T>>> {
            let value = match self.pending.as_mut().poll(cx) {
                Poll::Ready(value) => value?,
                Poll::Pending if finish => return Poll::Ready(Ok(None)),
                Poll::Pending => return Poll::Pending,
            };
            let callback = self
                .callback
                .take()
                .ok_or_else(|| wasmtime::format_err!("HTTP future produced twice"))?;
            Poll::Ready(callback(store.data_mut(), value).map(Some))
        }
    }
    FutureReader::new(
        store,
        Producer {
            pending: Box::pin(future),
            callback: Some(Box::new(callback)),
            marker: PhantomData,
        },
    )
}
