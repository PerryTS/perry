//! Private owned async values retain the exact instantiated canonical type.
//! Tokens are Store-local capabilities; validation never asks the engine to
//! discover a payload mismatch after ownership has already transferred.
use super::control::{Control, Operation};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wasmtime::{
    component::{Func, Linker, Type, Val},
    Store,
};

#[derive(Clone)]
pub(crate) struct Adapter {
    pub read: Func,
    pub dispose: Func,
    pub ty: Type,
    pub list: Type,
    pub schema: perry_wit::Type,
    pub writer: super::writers::WriterFunctions,
}
impl Adapter {
    pub fn new(
        store: &Store<crate::StoreData>,
        read: Func,
        dispose: Func,
        schema: perry_wit::Type,
        writer: super::writers::WriterFunctions,
    ) -> Result<Self> {
        let signature = read.ty(store);
        let params = signature.params().collect::<Vec<_>>();
        let signature = dispose.ty(store);
        let inputs = signature.params().collect::<Vec<_>>();
        ensure!(
            params.len() == 3 && inputs.len() == 1,
            "invalid private async adapter signature"
        );
        Ok(Self {
            read,
            dispose,
            ty: params[0].1.clone(),
            list: inputs[0].1.clone(),
            schema,
            writer,
        })
    }
}
struct Entry {
    ty: Type,
    reader: Option<Val>,
    pending: Option<u32>,
    cancelled: bool,
    end: bool,
}
pub(crate) struct Values {
    pub(super) nonce: u64,
    next: u64,
    pub(super) reserved: usize,
    pub(super) writers: BTreeMap<String, super::writers::Entry>,
    pub adapters: Vec<Arc<Adapter>>,
    entries: BTreeMap<String, Entry>,
    pub(super) control: Control,
}
pub(crate) struct Read {
    pub token: String,
    pub adapter: Arc<Adapter>,
    pub reader: Val,
    pub operation: Operation,
}
pub(crate) enum Begin {
    End,
    Read(Read),
}

pub(crate) fn token(value: &Value) -> Result<(&str, bool)> {
    let object = value.as_object().context("expected owned async value")?;
    ensure!(object.len() == 1, "invalid owned async value");
    if let Some(token) = value["$future"].as_str() {
        Ok((token, false))
    } else {
        Ok((
            value["$stream"]
                .as_str()
                .context("expected owned future or stream")?,
            true,
        ))
    }
}
impl Values {
    pub fn new() -> Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nonce = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| anyhow::anyhow!("async component identity exhausted"))?
            + 1;
        Ok(Self {
            nonce,
            next: 0,
            reserved: 0,
            writers: BTreeMap::new(),
            adapters: Vec::new(),
            entries: BTreeMap::new(),
            control: Control::default(),
        })
    }
    pub(super) fn capacity(&self, additional: usize) -> Result<()> {
        ensure!(
            self.entries.len() + self.writers.len() + self.reserved + additional <= 1024,
            "too many live component async values"
        );
        Ok(())
    }
    pub(super) fn fresh_token(&mut self) -> Result<String> {
        self.next = self
            .next
            .checked_add(1)
            .context("async value identity exhausted")?;
        Ok(format!("{}:{}", self.nonce, self.next))
    }
    pub fn link(&self, linker: &mut Linker<crate::StoreData>, name: &str) -> Result<()> {
        self.control.link(linker, name)
    }
    pub fn insert(&mut self, value: &Val, ty: &Type) -> Result<Value> {
        self.capacity(1)?;
        ensure!(
            self.adapters.iter().any(|a| a.ty == *ty),
            "no canonical adapter for async payload type"
        );
        let stream = match (value, ty) {
            (Val::Future(_), Type::Future(_)) => false,
            (Val::Stream(_), Type::Stream(_)) => true,
            _ => anyhow::bail!("async canonical value/type mismatch"),
        };
        let token = self.fresh_token()?;
        self.entries.insert(
            token.clone(),
            Entry {
                ty: ty.clone(),
                reader: Some(value.clone()),
                pending: None,
                cancelled: false,
                end: false,
            },
        );
        Ok(if stream {
            json!({"$stream":token})
        } else {
            json!({"$future":token})
        })
    }
    pub fn get(&self, value: &Value, ty: &Type, transfers: &mut BTreeSet<String>) -> Result<Val> {
        let (token, stream) = token(value)?;
        ensure!(
            stream == matches!(ty, Type::Stream(_)),
            "async value kind mismatch"
        );
        let entry = self
            .entries
            .get(token)
            .context("async value is disposed, transferred, or belongs to another component")?;
        ensure!(entry.ty == *ty, "async payload type mismatch");
        ensure!(
            entry.pending.is_none() && !entry.end,
            "async value is busy or closed"
        );
        ensure!(
            transfers.insert(token.to_string()),
            "async value is transferred more than once"
        );
        entry.reader.clone().context("async value is not readable")
    }
    pub fn transferred(&mut self, ids: &BTreeSet<String>) {
        for id in ids {
            self.entries.remove(id);
        }
    }
    pub fn begin(&mut self, value: &Value, capacity: u32) -> Result<Begin> {
        let (token, stream) = token(value)?;
        ensure!(
            (1..=65536).contains(&capacity) && (stream || capacity == 1),
            "invalid async read capacity"
        );
        let entry = self
            .entries
            .get(token)
            .context("async value is disposed, transferred, or belongs to another component")?;
        ensure!(
            stream == matches!(entry.ty, Type::Stream(_)),
            "async value kind mismatch"
        );
        ensure!(
            entry.pending.is_none(),
            "async value already has a pending read"
        );
        if entry.end {
            self.entries.remove(token);
            return Ok(Begin::End);
        }
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.ty == entry.ty)
            .context("missing async payload adapter")?
            .clone();
        let operation = self.control.reserve()?;
        let entry = self.entries.get_mut(token).unwrap();
        let reader = entry.reader.take().context("async value is not readable")?;
        entry.pending = Some(operation.id);
        Ok(Begin::Read(Read {
            token: token.to_string(),
            adapter,
            reader,
            operation,
        }))
    }
    pub fn request_cancel(&mut self, read: &Read) -> Result<()> {
        let entry = self
            .entries
            .get_mut(&read.token)
            .context("canonical read token disappeared")?;
        ensure!(
            entry.pending == Some(read.operation.id),
            "stale canonical read cancellation"
        );
        entry.cancelled = true;
        Ok(())
    }
    pub fn finish(
        &mut self,
        read: &Read,
        status: u32,
        count: u32,
        reader: Option<Val>,
    ) -> Result<bool> {
        let entry = self
            .entries
            .get_mut(&read.token)
            .context("canonical read token disappeared")?;
        ensure!(
            entry.pending == Some(read.operation.id),
            "stale canonical read completion"
        );
        let cancelled = entry.cancelled || status == 2;
        if cancelled || matches!(entry.ty, Type::Future(_)) || (status == 1 && count == 0) {
            self.entries.remove(&read.token);
            return Ok(cancelled);
        }
        entry.pending = None;
        if status == 0 {
            ensure!(
                matches!(reader, Some(Val::Stream(_))),
                "missing stream continuation"
            );
            entry.reader = reader;
        } else {
            ensure!(reader.is_none(), "unexpected stream continuation");
            entry.end = true;
        }
        Ok(false)
    }
    pub fn collect(&mut self, value: &Value) -> Result<Option<Val>> {
        let (token, _) = token(value)?;
        if !self.entries.contains_key(token) {
            return Ok(None);
        }
        self.close(value)
    }
    pub fn close(&mut self, value: &Value) -> Result<Option<Val>> {
        let (token, stream) = token(value)?;
        let entry = self
            .entries
            .get_mut(token)
            .context("async value is disposed, transferred, or belongs to another component")?;
        ensure!(
            stream == matches!(entry.ty, Type::Stream(_)),
            "async value kind mismatch"
        );
        if let Some(operation) = entry.pending {
            entry.cancelled = true;
            self.control.cancel(operation)?;
            return Ok(None);
        }
        Ok(self.entries.remove(token).unwrap().reader)
    }
}
