use std::num::{NonZeroU32,NonZeroU64};
#[derive(Clone,Copy)]
pub struct Triple { bytes:NonZeroU32, units:u32, source:u32 }
#[derive(Clone,Copy)]
pub struct Pair { lengths:NonZeroU64, source:u32 }
#[unsafe(no_mangle)]
#[inline(never)]
pub fn triple(bytes:u32,units:u32,source:u32)->Option<Triple>{Some(Triple{bytes:NonZeroU32::new(bytes)?,units,source})}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn pair(bytes:u32,units:u32,source:u32)->Option<Pair>{Some(Pair{lengths:NonZeroU64::new(bytes as u64 | ((units as u64)<<32))?,source})}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn use_triple(bytes:u32,units:u32,source:u32)->u64{match triple(bytes,units,source){Some(t)=>(t.bytes.get() as u64).wrapping_add(t.units as u64).wrapping_add(t.source as u64),None=>0}}
#[unsafe(no_mangle)]
#[inline(never)]
pub fn use_pair(bytes:u32,units:u32,source:u32)->u64{match pair(bytes,units,source){Some(t)=>(t.lengths.get() as u32 as u64).wrapping_add(t.lengths.get()>>32).wrapping_add(t.source as u64),None=>0}}
