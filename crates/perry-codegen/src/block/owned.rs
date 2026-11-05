//! Owned transport of a completed lowering graph. No reference-counted
//! builder handles cross the worker boundary. Exhaustive destructuring keeps
//! newly added builder fields visible to this conversion.

use super::*;

pub(crate) struct OwnedBlock {
    label: String,
    instructions: Vec<crate::inst::LlInst>,
    terminated: bool,
    fp_flags: FpFlags,
    i64_from_double: HashMap<String, String>,
    double_from_i64: HashMap<String, String>,
}

impl LlBlock {
    pub(crate) fn into_owned(self) -> OwnedBlock {
        let Self {
            label,
            instructions,
            terminated,
            counter,
            fp_flags,
            i64_from_double,
            double_from_i64,
        } = self;
        drop(counter);
        OwnedBlock {
            label,
            instructions,
            terminated,
            fp_flags,
            i64_from_double,
            double_from_i64,
        }
    }
}

impl OwnedBlock {
    pub(crate) fn into_builder(self, counter: Rc<RegCounter>) -> LlBlock {
        let Self {
            label,
            instructions,
            terminated,
            fp_flags,
            i64_from_double,
            double_from_i64,
        } = self;

        LlBlock {
            label,
            instructions,
            terminated,
            counter,
            fp_flags,
            i64_from_double,
            double_from_i64,
        }
    }
}
