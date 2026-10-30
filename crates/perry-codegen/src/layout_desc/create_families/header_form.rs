//! `HeaderForm` answers for the family records, split out of `create_families`
//! to keep that file under the 2000-line cap. These are inherent impls on the
//! module's own types, so a child module carries them without re-export.
use super::*;

impl Site {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

impl Create {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}

impl Add {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        self.family.form()
    }
}

impl HolderRead {
    /// The header form predicted for the objects this record describes (`HeaderForm`:
    /// every ordinary object has the meta word today). Sites take their slot base
    /// from here, never from the type byte.
    pub(crate) fn form(&self) -> crate::target_layout::HeaderForm {
        crate::target_layout::HeaderForm::Meta
    }
}
