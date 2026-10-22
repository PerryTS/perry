//! Linker-retention anchors generated from the checked-in ABI inventory.
//!
//! The table names every exported Node-API entry point. It is referenced only
//! from the host's install ([`retain`]), so a program that installs the host
//! (one with an approved addon) keeps the whole ABI surface, and any other
//! program links none of it. perry's link step also exports each symbol for
//! such a program (`add_node_api_host_link_args`).

#[allow(dead_code)] // the anchor only needs the symbol address
struct NodeApiSymbol(*const ());
unsafe impl Sync for NodeApiSymbol {}

include!(concat!(env!("OUT_DIR"), "/node_api_host_symbol_anchors.rs"));

/// Keep the Node-API exports in a program that installs the host.
pub(super) fn retain() {
    std::hint::black_box(&NODE_API_HOST_SYMBOL_ANCHORS);
}
