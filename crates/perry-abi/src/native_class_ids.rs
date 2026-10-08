//! Shared class ids for externally implemented payload families.
pub const GZIP: u32 = 0xFFFF_241D;
pub const GUNZIP: u32 = 0xFFFF_241E;
pub const DEFLATE: u32 = 0xFFFF_241F;
pub const INFLATE: u32 = 0xFFFF_2420;
pub const DEFLATE_RAW: u32 = 0xFFFF_2421;
pub const INFLATE_RAW: u32 = 0xFFFF_2422;
pub const UNZIP: u32 = 0xFFFF_2423;
pub const BROTLI_COMPRESS: u32 = 0xFFFF_2424;
pub const BROTLI_DECOMPRESS: u32 = 0xFFFF_2425;
pub const ZSTD_COMPRESS: u32 = 0xFFFF_2426;
pub const ZSTD_DECOMPRESS: u32 = 0xFFFF_2427;
pub const ZLIB_BASE: u32 = 0xFFFF_2428;
pub const HTTP_SERVER_RESPONSE: u32 = 0xFFFF_2429;
