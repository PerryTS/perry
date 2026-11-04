//! Utility-package stdlib FFI declarations (extracted from stdlib_ffi.rs):
//! @perryts/pdf, decimal.js, ethers.

use crate::module::LlModule;
use crate::types::{DOUBLE, I64, VOID};

pub(crate) fn declare_utilities(module: &mut LlModule) {
    // ========== @perryts/pdf (issue #516) ==========
    // createPdf returns an i64 handle (NaN-boxed POINTER_TAG by
    // codegen via NR_PTR). The mutator ops are Rust `-> ()` and
    // therefore VOID at the LLVM ABI level.
    module.declare_function("js_pdf_create_pdf", I64, &[DOUBLE]);
    module.declare_function("js_pdf_add_text", VOID, &[I64, I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_pdf_add_line",
        VOID,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_pdf_new_page", VOID, &[I64]);
    module.declare_function("js_pdf_save", VOID, &[I64]);

    // ========== Ethers / blockchain ==========
    module.declare_function("js_ethers_format_ether", I64, &[I64]);
    module.declare_function("js_ethers_format_units", I64, &[I64, DOUBLE]);
    module.declare_function("js_ethers_get_address", I64, &[I64]);
    module.declare_function("js_ethers_parse_ether", I64, &[I64]);
    module.declare_function("js_ethers_parse_units", I64, &[I64, DOUBLE]);
}
