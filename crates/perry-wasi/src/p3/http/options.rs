// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception.
use std::{ops::Deref, sync::Arc};
pub enum MaybeMutable<T> {
    /// Clone-on-write, mutable [Arc]
    Mutable(Arc<T>),
    /// Immutable [Arc]
    Immutable(Arc<T>),
}

impl<T> From<MaybeMutable<T>> for Arc<T> {
    fn from(v: MaybeMutable<T>) -> Self {
        v.into_arc()
    }
}

impl<T> Deref for MaybeMutable<T> {
    type Target = Arc<T>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Mutable(v) | Self::Immutable(v) => v,
        }
    }
}

impl<T> MaybeMutable<T> {
    /// Construct a mutable [`MaybeMutable`].
    pub fn new_mutable(v: impl Into<Arc<T>>) -> Self {
        Self::Mutable(v.into())
    }

    /// Construct a mutable [`MaybeMutable`] filling it with default `T`.
    pub fn new_mutable_default() -> Self
    where
        T: Default,
    {
        Self::new_mutable(T::default())
    }

    /// Construct an immutable [`MaybeMutable`].
    pub fn new_immutable(v: impl Into<Arc<T>>) -> Self {
        Self::Immutable(v.into())
    }

    /// Unwrap [`MaybeMutable`] into [`Arc`].
    pub fn into_arc(self) -> Arc<T> {
        match self {
            Self::Mutable(v) | Self::Immutable(v) => v,
        }
    }

    /// If this [`MaybeMutable`] is [`Mutable`](MaybeMutable::Mutable),
    /// return a mutable reference to it, otherwise return `None`.
    ///
    /// Internally, this will use [`Arc::make_mut`] and will clone the underlying
    /// value, if multiple strong references to the inner [`Arc`] exist.
    pub fn get_mut(&mut self) -> Option<&mut T>
    where
        T: Clone,
    {
        match self {
            Self::Mutable(v) => Some(Arc::make_mut(v)),
            Self::Immutable(..) => None,
        }
    }
}
