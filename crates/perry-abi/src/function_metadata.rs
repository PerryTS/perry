//! Function metadata is indexed by an explicit image owner and entry ordinal.
use crate::{JsFunctionInfo, FN_CALL_ENTRY_SHIFT, FN_HAS_CONSTRUCTOR_LAYOUT, FN_HAS_NAME};
use core::{ffi::c_void, sync::atomic::AtomicPtr};

/// Licenses a final `FunctionMetadataIdentity` tail, after all existing tails.
pub const FN_HAS_METADATA: u32 = 1 << 25;
/// The owner and all its relative info records are permanent image data.
pub const METADATA_PERMANENT: u32 = 1;

/// One image's canonical entry inventory. Each signed entry is relative to
/// `entries` and names the info that owns its name/source. Runtime-native
/// owners have one entry and a null entries pointer. `state` owns copied
/// metadata and sparse overrides; null means no mutable payload exists.
#[repr(C)]
pub struct FunctionMetadataOwner {
    pub entries: *const i32,
    pub count: u32,
    pub flags: u32,
    pub state: AtomicPtr<c_void>,
}
unsafe impl Sync for FunctionMetadataOwner {}
impl FunctionMetadataOwner {
    pub const fn native() -> Self {
        Self {
            entries: core::ptr::null(),
            count: 1,
            flags: 0,
            state: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

/// Indexing never derives identity from a code or owner address.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FunctionMetadataIdentity {
    pub owner: *const FunctionMetadataOwner,
    pub ordinal: u32,
}
unsafe impl Sync for FunctionMetadataIdentity {}

impl JsFunctionInfo {
    /// Optional metadata tail. The existing source/name/constructor and generic
    /// call-entry offsets retain their meanings. An invalid ordinal is absent.
    pub fn metadata_identity(&self) -> Option<FunctionMetadataIdentity> {
        if self.flags & FN_HAS_METADATA == 0 {
            return None;
        }
        let entry_words = self.flags >> FN_CALL_ENTRY_SHIFT;
        let offset = if entry_words != 0 {
            entry_words as usize * 8 + core::mem::size_of::<*const u8>()
        } else {
            core::mem::size_of::<Self>()
                + crate::function_source_tail_size(self)
                + 8 * usize::from(self.flags & FN_HAS_NAME != 0)
                + if self.flags & FN_HAS_CONSTRUCTOR_LAYOUT != 0 {
                    core::mem::size_of::<(*const u8, *const u8, u32, u32)>()
                } else {
                    0
                }
        };
        // SAFETY: the flag licenses this tail. The defining image/owner lives
        // at least as long as the info, exactly like its other optional tails.
        unsafe {
            let identity = (self as *const Self)
                .cast::<u8>()
                .add(offset)
                .cast::<FunctionMetadataIdentity>()
                .read();
            let owner = identity.owner.as_ref()?;
            (identity.ordinal < owner.count).then_some(identity)
        }
    }
}

/// A native declaration explicitly owns one canonical entry. Multiple infos
/// for a shared thunk can name the same owner; per-closure own names remain
/// properties of their closures. Never register native owners as stack code.
#[repr(C)]
pub struct NativeFunctionInfo {
    pub info: JsFunctionInfo,
    pub identity: FunctionMetadataIdentity,
}
impl NativeFunctionInfo {
    pub const fn new(info: JsFunctionInfo, owner: &'static FunctionMetadataOwner) -> Self {
        Self {
            info: info.with_flags(FN_HAS_METADATA),
            identity: FunctionMetadataIdentity { owner, ordinal: 0 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_follows_every_licensed_optional_tail() {
        static OWNER: FunctionMetadataOwner = FunctionMetadataOwner::native();
        for source_tail in [0, 8, 16] {
            for name in [false, true] {
                for constructor in [false, true] {
                    for call in [false, true] {
                        let mut storage = [0u64; 24];
                        let mut info = unsafe { JsFunctionInfo::from_code(1 as *const u8, 0) }
                            .with_flags(FN_HAS_METADATA);
                        if source_tail != 0 {
                            info.flags |= crate::FN_HAS_SOURCE;
                            info.versioned_code = 1 as *const u8;
                            if source_tail == 16 {
                                info.flags |= crate::FN_SOURCE_BLOCKS;
                            }
                        }
                        if name {
                            info.flags |= FN_HAS_NAME;
                        }
                        if constructor {
                            info.flags |= FN_HAS_CONSTRUCTOR_LAYOUT;
                        }
                        let mut offset = core::mem::size_of::<JsFunctionInfo>()
                            + source_tail
                            + 8 * usize::from(name)
                            + if constructor {
                                core::mem::size_of::<(*const u8, *const u8, u32, u32)>()
                            } else {
                                0
                            };
                        if call {
                            info.flags |= ((offset / 8) as u32) << FN_CALL_ENTRY_SHIFT;
                            offset += core::mem::size_of::<*const u8>();
                        }
                        unsafe {
                            let base = storage.as_mut_ptr().cast::<u8>();
                            base.cast::<JsFunctionInfo>().write(info);
                            base.add(offset).cast::<FunctionMetadataIdentity>().write(
                                FunctionMetadataIdentity {
                                    owner: &OWNER,
                                    ordinal: 0,
                                },
                            );
                            let identity = (&*base.cast::<JsFunctionInfo>())
                                .metadata_identity()
                                .unwrap();
                            assert_eq!(identity.owner, &OWNER as *const _);
                            assert_eq!(identity.ordinal, 0);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn layouts_and_native_identity_are_pinned() {
        assert_eq!(
            core::mem::offset_of!(FunctionMetadataOwner, count),
            core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::offset_of!(FunctionMetadataIdentity, ordinal),
            core::mem::size_of::<usize>()
        );
        assert_eq!(
            core::mem::offset_of!(NativeFunctionInfo, identity),
            core::mem::size_of::<JsFunctionInfo>()
        );
        static OWNER: FunctionMetadataOwner = FunctionMetadataOwner::native();
        let mut native = NativeFunctionInfo::new(
            unsafe { JsFunctionInfo::from_code(1 as *const u8, 0) },
            &OWNER,
        );
        assert_eq!(native.info.metadata_identity().unwrap().ordinal, 0);
        native.identity.ordinal = 1;
        assert!(native.info.metadata_identity().is_none());
        assert!(unsafe { JsFunctionInfo::from_code(1 as *const u8, 0) }
            .metadata_identity()
            .is_none());
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(core::mem::size_of::<FunctionMetadataOwner>(), 24);
            assert_eq!(core::mem::size_of::<FunctionMetadataIdentity>(), 16);
            assert_eq!(core::mem::size_of::<NativeFunctionInfo>(), 80);
        }
    }
}
