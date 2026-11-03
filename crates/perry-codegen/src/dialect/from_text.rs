//! Whole-function entry points of the text reader.

use anyhow::{anyhow, Result};
use inkwell::context::Context;
use inkwell::module::Module;

use super::numbered::NumberedMetadata;
use super::FnStream;

/// Parse `fn_text` (a complete `define ... { ... }`) and build it into
/// `module`. Returns the number of instructions constructed.
#[allow(dead_code)] // retained as the text-path oracle for native-emission debugging
pub(crate) fn add_function_from_text<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    fn_text: &str,
) -> Result<usize> {
    add_function_from_text_with(context, module, &NumberedMetadata::none(), fn_text)
}

/// [`add_function_from_text`] with the module's numbered metadata.
#[allow(dead_code)]
pub(crate) fn add_function_from_text_with<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    metadata: &NumberedMetadata<'ctx>,
    fn_text: &str,
) -> Result<usize> {
    let mut lines = fn_text.lines();
    let header = lines.next().ok_or_else(|| anyhow!("empty function text"))?;
    let mut stream = FnStream::begin(context, module, metadata, header)?;
    for line in lines {
        stream.line(line)?;
    }
    stream.finish().map(|(t, r)| t + r)
}
