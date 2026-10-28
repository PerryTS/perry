//! Store-local writer ownership and factory reservations. No core table handle
//! is exposed to application code; native descriptors retain exact types.
use super::{
    control::Operation,
    values::{Adapter, Values},
};
use anyhow::{ensure, Context, Result};
use perry_wit::Type as Wit;
use serde_json::{json, Value};
use std::sync::Arc;
use wasmtime::component::{Func, Type, Val};

/// A component-bound factory type. Payload aliases refer to the selected WIT
/// world's type definitions; the engine's instantiated type stays private.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncValueType {
    owner: u64,
    index: usize,
    schema: Wit,
}
impl AsyncValueType {
    pub fn wit(&self) -> &Wit {
        &self.schema
    }
    pub fn is_stream(&self) -> bool {
        matches!(self.schema, Wit::Stream(_))
    }
    pub fn payload(&self) -> Option<&Wit> {
        match &self.schema {
            Wit::Future(t) | Wit::Stream(t) => t.as_deref(),
            _ => unreachable!(),
        }
    }
}
#[derive(Clone)]
pub(crate) struct WriterFunctions {
    pub new: Func,
    pub write: Func,
    pub close: Func,
    pub dispose: Func,
    pub list: Type,
}
pub(super) struct Entry {
    index: usize,
    core: u32,
    pending: Option<u32>,
    cancelled: bool,
    close: bool,
}
pub(crate) struct Write {
    pub token: String,
    pub adapter: Arc<Adapter>,
    pub core: u32,
    pub operation: Operation,
}
pub(crate) struct WriterHandle {
    pub adapter: Arc<Adapter>,
    pub core: u32,
}
pub(crate) struct Written {
    pub cancelled: bool,
    pub removed: bool,
    pub discard: bool,
    pub close: Option<WriterHandle>,
}
pub(crate) fn token(value: &Value) -> Result<&str> {
    let object = value.as_object().context("expected owned async writer")?;
    ensure!(object.len() == 1, "invalid owned async writer");
    object
        .get("$writer")
        .and_then(Value::as_str)
        .context("expected owned async writer")
}
impl Values {
    pub fn writer_metadata(&self) -> Vec<(String, bool)> {
        self.writers
            .iter()
            .map(|(token, entry)| {
                (
                    token.clone(),
                    matches!(self.adapters[entry.index].ty, Type::Stream(_)),
                )
            })
            .collect()
    }
    pub fn types(&self) -> Vec<AsyncValueType> {
        self.adapters
            .iter()
            .enumerate()
            .map(|(index, a)| AsyncValueType {
                owner: self.nonce,
                index,
                schema: a.schema.clone(),
            })
            .collect()
    }
    pub fn reserve_factory(&mut self, ty: &AsyncValueType) -> Result<(usize, Arc<Adapter>)> {
        ensure!(
            ty.owner == self.nonce,
            "async factory type belongs to another component"
        );
        let adapter = self
            .adapters
            .get(ty.index)
            .context("unknown async factory type")?
            .clone();
        self.capacity(2)?;
        self.reserved += 2;
        Ok((ty.index, adapter))
    }
    pub fn factory_result(&mut self, index: usize, pair: &Val) -> Result<(Value, String, bool)> {
        let Val::Tuple(pair) = pair else {
            anyhow::bail!("invalid canonical factory result")
        };
        let [reader, Val::U32(core)] = pair.as_slice() else {
            anyhow::bail!("invalid canonical factory pair")
        };
        let ty = self.adapters[index].ty.clone();
        self.reserved -= 2;
        let reader = self.insert(reader, &ty)?;
        let writer = self.fresh_token()?;
        self.writers.insert(
            writer.clone(),
            Entry {
                index,
                core: *core,
                pending: None,
                cancelled: false,
                close: false,
            },
        );
        Ok((
            json!({"value":reader,"writer":{"$writer":writer}}),
            writer,
            matches!(ty, Type::Stream(_)),
        ))
    }
    pub fn prepare_write(&self, value: &Value) -> Result<Write> {
        let token = token(value)?;
        let entry = self
            .writers
            .get(token)
            .context("writer is closed or belongs to another component")?;
        ensure!(
            entry.pending.is_none(),
            "writer already has a pending write"
        );
        Ok(Write {
            token: token.into(),
            adapter: self.adapters[entry.index].clone(),
            core: entry.core,
            operation: self.control.reserve()?,
        })
    }
    pub fn commit_write(&mut self, write: &Write) {
        self.writers.get_mut(&write.token).unwrap().pending = Some(write.operation.id);
    }
    pub fn signal_write(&self, write: &Write) -> Result<()> {
        self.control.cancel(write.operation.id)
    }
    pub fn finish_write(
        &mut self,
        write: &Write,
        status: u32,
        count: u32,
        cancelled: bool,
        close: bool,
    ) -> Result<Written> {
        let entry = self
            .writers
            .get_mut(&write.token)
            .context("canonical writer disappeared")?;
        ensure!(
            entry.pending == Some(write.operation.id),
            "stale canonical writer completion"
        );
        let cancelled = cancelled || entry.cancelled || status == 2;
        let close = close || entry.close;
        let dropped = status == 1 || (matches!(write.adapter.ty, Type::Future(_)) && count == 1);
        let removed = dropped || close;
        let closing = if close && !dropped {
            Some(WriterHandle {
                adapter: write.adapter.clone(),
                core: entry.core,
            })
        } else {
            None
        };
        if removed {
            self.writers.remove(&write.token);
        } else {
            entry.pending = None;
            entry.cancelled = false;
            entry.close = false;
        }
        Ok(Written {
            cancelled,
            removed,
            discard: close,
            close: closing,
        })
    }
    /// Cancellation targets only operations registered when the request was
    /// submitted. A delayed cancel cannot cancel a subsequent retry.
    pub fn control_writer(
        &mut self,
        value: &Value,
        close: bool,
        operations: &[u32],
    ) -> Result<Option<WriterHandle>> {
        let token = token(value)?;
        let entry = self
            .writers
            .get_mut(token)
            .context("writer is closed or belongs to another component")?;
        let adapter = self.adapters[entry.index].clone();
        ensure!(
            !close || matches!(adapter.ty, Type::Stream(_)),
            "an unwritten future writer must be completed or explicitly aborted"
        );
        if let Some(operation) = entry.pending {
            if close || operations.contains(&operation) {
                entry.cancelled = true;
                entry.close |= close;
                self.control.cancel(operation)?;
            }
            return Ok(None);
        }
        if !close {
            return Ok(None);
        }
        let entry = self.writers.remove(token).unwrap();
        Ok(Some(WriterHandle {
            adapter,
            core: entry.core,
        }))
    }
    pub fn has_writer(&self, value: &Value) -> Result<()> {
        ensure!(
            self.writers.contains_key(token(value)?),
            "writer is closed or belongs to another component"
        );
        Ok(())
    }
}
