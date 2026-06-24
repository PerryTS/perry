use super::*;

mod assert;
mod dgram_fs_os;
mod inspector_vm;
mod module_sea_tls_test;
mod url_punycode_console;
mod util_buffer;

use assert::NODE_CORE_ASSERT_ROWS;
use dgram_fs_os::NODE_CORE_DGRAM_FS_OS_ROWS;
use inspector_vm::NODE_CORE_INSPECTOR_VM_ROWS;
use module_sea_tls_test::NODE_CORE_MODULE_SEA_TLS_TEST_ROWS;
use url_punycode_console::NODE_CORE_URL_PUNYCODE_CONSOLE_ROWS;
use util_buffer::NODE_CORE_UTIL_BUFFER_ROWS;

/// Total row count across all node-core sub-tables. Iteration order is
/// stable and matches the pre-split single-file declaration order —
/// important for `iter_native_module_table` and the downstream
/// `perry-api-manifest` drift gate (#512).
const NODE_CORE_ROWS_LEN: usize = NODE_CORE_INSPECTOR_VM_ROWS.len()
    + NODE_CORE_MODULE_SEA_TLS_TEST_ROWS.len()
    + NODE_CORE_DGRAM_FS_OS_ROWS.len()
    + NODE_CORE_URL_PUNYCODE_CONSOLE_ROWS.len()
    + NODE_CORE_ASSERT_ROWS.len()
    + NODE_CORE_UTIL_BUFFER_ROWS.len();

/// Concatenate the per-topic node-core row slices into one fixed-size
/// array at const time. `NativeModSig` is `Copy`, so we can copy each
/// element by index. Order is preserved exactly as in the original
/// single-file table.
const fn concat_node_core_rows() -> [NativeModSig; NODE_CORE_ROWS_LEN] {
    // The first row of the first (always non-empty) sub-table is used as
    // the array filler; every slot is overwritten below.
    let mut out = [NODE_CORE_INSPECTOR_VM_ROWS[0]; NODE_CORE_ROWS_LEN];
    let mut idx = 0;

    let groups: [&[NativeModSig]; 6] = [
        NODE_CORE_INSPECTOR_VM_ROWS,
        NODE_CORE_MODULE_SEA_TLS_TEST_ROWS,
        NODE_CORE_DGRAM_FS_OS_ROWS,
        NODE_CORE_URL_PUNYCODE_CONSOLE_ROWS,
        NODE_CORE_ASSERT_ROWS,
        NODE_CORE_UTIL_BUFFER_ROWS,
    ];

    let mut g = 0;
    while g < groups.len() {
        let group = groups[g];
        let mut i = 0;
        while i < group.len() {
            out[idx] = group[i];
            idx += 1;
            i += 1;
        }
        g += 1;
    }

    out
}

const NODE_CORE_ROWS_ARR: [NativeModSig; NODE_CORE_ROWS_LEN] = concat_node_core_rows();

pub(super) const NODE_CORE_ROWS: &[NativeModSig] = &NODE_CORE_ROWS_ARR;
