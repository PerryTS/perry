//! Cancellation signals are native channels scoped to one canonical operation.
use anyhow::{ensure, Context, Result};
use futures_channel::oneshot;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use wasmtime::component::{Linker, Val};

struct Signal {
    send: Option<oneshot::Sender<()>>,
    receive: Option<oneshot::Receiver<()>>,
}
#[derive(Default)]
struct Inner {
    next: u32,
    entries: BTreeMap<u32, Signal>,
}
#[derive(Clone, Default)]
pub(super) struct Control(Arc<Mutex<Inner>>);
pub(crate) struct Operation {
    pub id: u32,
    control: Control,
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.control.0.lock().unwrap().entries.remove(&self.id);
    }
}
impl Control {
    pub fn reserve(&self) -> Result<Operation> {
        let mut inner = self.0.lock().unwrap();
        let id = inner
            .next
            .checked_add(1)
            .context("canonical cancellation identity exhausted")?;
        inner.next = id;
        let (send, receive) = oneshot::channel();
        inner.entries.insert(
            id,
            Signal {
                send: Some(send),
                receive: Some(receive),
            },
        );
        Ok(Operation {
            id,
            control: self.clone(),
        })
    }
    pub fn cancel(&self, id: u32) -> Result<()> {
        let mut inner = self.0.lock().unwrap();
        let entry = inner
            .entries
            .get_mut(&id)
            .context("canonical read is no longer pending")?;
        if let Some(send) = entry.send.take() {
            let _ = send.send(());
        }
        Ok(())
    }
    pub fn link(&self, linker: &mut Linker<crate::StoreData>, name: &str) -> Result<()> {
        let control = self.clone();
        linker
            .instance(name)?
            .func_new_concurrent("wait", move |_, _, args, _| {
                let control = control.clone();
                Box::pin(async move {
                    let result = async {
                        let [Val::U32(id)] = args else {
                            anyhow::bail!("invalid canonical control operation")
                        };
                        let receive = control
                            .0
                            .lock()
                            .unwrap()
                            .entries
                            .get_mut(id)
                            .context("unknown canonical read operation")?
                            .receive
                            .take()
                            .context("canonical signal already awaited")?;
                        ensure!(receive.await.is_ok(), "canonical read operation retired");
                        Ok(())
                    }
                    .await;
                    result.map_err(wasmtime::Error::from_anyhow)
                })
            })?;
        Ok(())
    }
}
