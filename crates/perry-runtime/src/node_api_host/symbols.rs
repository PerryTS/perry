//! Linker-retention anchors generated from the checked-in ABI inventory.

struct NodeApiSymbol(*const ());
unsafe impl Sync for NodeApiSymbol {}

include!(concat!(env!("OUT_DIR"), "/node_api_host_symbol_anchors.rs"));
