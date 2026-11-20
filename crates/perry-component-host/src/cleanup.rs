//! Deferred cleanup checks the original ownership identity, not just a numeric
//! representation that may now belong to a later owner.
use crate::{Host, ResourceRef};
use anyhow::Result;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum CollectedResource {
    Guest { token: String },
    Host { resource: ResourceRef },
}

impl Host {
    pub(crate) fn collect_resource(
        &mut self,
        request: CollectedResource,
        released: &mut bool,
    ) -> Result<()> {
        let Some(live) = self.live.as_ref() else {
            return Ok(());
        };
        let token = match request {
            CollectedResource::Guest { token } => {
                // Transfer removes the old token. A late cleanup job must not
                // take ownership back from the guest or a new returned wrapper.
                if !live.store.data().resources.owns(&token) {
                    return Ok(());
                }
                token
            }
            CollectedResource::Host { resource } => {
                let Some(entry) = live
                    .store
                    .data()
                    .host_resources
                    .entries
                    .get(&resource.token)
                else {
                    return Ok(());
                };
                if entry.reference != resource || entry.handle.is_none() {
                    return Ok(());
                }
                resource.token
            }
        };
        self.drop_resource_tracked(&token, released)
    }
}
