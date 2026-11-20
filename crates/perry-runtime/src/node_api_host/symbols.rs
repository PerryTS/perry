//! Compile-time check of the checked-in ABI inventory
//! (`perry-runtime-manifest/src/node_api_host_symbols.txt`): every line names
//! a host definition. Nothing references the table, so no program links it.
//! A native addon reaches what its Node-API imports reach: perry's link step
//! retains and exports the symbols the program's addons import, and the
//! whole inventory for an open addon (`add_node_api_host_link_args`).
#![allow(dead_code)]

struct NodeApiSymbol(*const ());
unsafe impl Sync for NodeApiSymbol {}

include!(concat!(env!("OUT_DIR"), "/node_api_host_symbol_anchors.rs"));
