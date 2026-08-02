/// Sorted table for `zlib_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static ZLIB_CONST_TABLE: &[(&str, f64)] = &[
    ("BROTLI_DECODE", (8) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES", (-30) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP", (-25) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES", (-21) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1", (-26) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2", (-27) as f64),
    ("BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS", (-22) as f64),
    ("BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET", (-19) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1", (-9) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2", (-10) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_CL_SPACE", (-6) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT",
        (-8) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_DICTIONARY", (-12) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_DISTANCE", (-16) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE",
        (-3) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE", (-1) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE", (-7) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_1", (-14) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_PADDING_2", (-15) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_RESERVED", (-2) as f64),
    (
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET",
        (-4) as f64,
    ),
    (
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME",
        (-5) as f64,
    ),
    ("BROTLI_DECODER_ERROR_FORMAT_TRANSFORM", (-11) as f64),
    ("BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS", (-13) as f64),
    ("BROTLI_DECODER_ERROR_INVALID_ARGUMENTS", (-20) as f64),
    ("BROTLI_DECODER_ERROR_UNREACHABLE", (-31) as f64),
    ("BROTLI_DECODER_NEEDS_MORE_INPUT", (2) as f64),
    ("BROTLI_DECODER_NEEDS_MORE_OUTPUT", (3) as f64),
    ("BROTLI_DECODER_NO_ERROR", (0) as f64),
    (
        "BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION",
        (0) as f64,
    ),
    ("BROTLI_DECODER_PARAM_LARGE_WINDOW", (1) as f64),
    ("BROTLI_DECODER_RESULT_ERROR", (0) as f64),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT", (2) as f64),
    ("BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT", (3) as f64),
    ("BROTLI_DECODER_RESULT_SUCCESS", (1) as f64),
    ("BROTLI_DECODER_SUCCESS", (1) as f64),
    ("BROTLI_DEFAULT_MODE", (0) as f64),
    ("BROTLI_DEFAULT_QUALITY", (11) as f64),
    ("BROTLI_DEFAULT_WINDOW", (22) as f64),
    ("BROTLI_ENCODE", (9) as f64),
    ("BROTLI_LARGE_MAX_WINDOW_BITS", (30) as f64),
    ("BROTLI_MAX_INPUT_BLOCK_BITS", (24) as f64),
    ("BROTLI_MAX_QUALITY", (11) as f64),
    ("BROTLI_MAX_WINDOW_BITS", (24) as f64),
    ("BROTLI_MIN_INPUT_BLOCK_BITS", (16) as f64),
    ("BROTLI_MIN_QUALITY", (0) as f64),
    ("BROTLI_MIN_WINDOW_BITS", (10) as f64),
    ("BROTLI_MODE_FONT", (2) as f64),
    ("BROTLI_MODE_GENERIC", (0) as f64),
    ("BROTLI_MODE_TEXT", (1) as f64),
    ("BROTLI_OPERATION_EMIT_METADATA", (3) as f64),
    ("BROTLI_OPERATION_FINISH", (2) as f64),
    ("BROTLI_OPERATION_FLUSH", (1) as f64),
    ("BROTLI_OPERATION_PROCESS", (0) as f64),
    ("BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING", (4) as f64),
    ("BROTLI_PARAM_LARGE_WINDOW", (6) as f64),
    ("BROTLI_PARAM_LGBLOCK", (3) as f64),
    ("BROTLI_PARAM_LGWIN", (2) as f64),
    ("BROTLI_PARAM_MODE", (0) as f64),
    ("BROTLI_PARAM_NDIRECT", (8) as f64),
    ("BROTLI_PARAM_NPOSTFIX", (7) as f64),
    ("BROTLI_PARAM_QUALITY", (1) as f64),
    ("BROTLI_PARAM_SIZE_HINT", (5) as f64),
    ("DEFLATE", (1) as f64),
    ("DEFLATERAW", (5) as f64),
    ("GUNZIP", (4) as f64),
    ("GZIP", (3) as f64),
    ("INFLATE", (2) as f64),
    ("INFLATERAW", (6) as f64),
    ("UNZIP", (7) as f64),
    ("ZLIB_VERNUM", (0x1310) as f64),
    ("ZSTD_CLEVEL_DEFAULT", (3) as f64),
    ("ZSTD_COMPRESS", (10) as f64),
    ("ZSTD_DECOMPRESS", (11) as f64),
    ("ZSTD_MAXCLEVEL", (22) as f64),
    ("ZSTD_MINCLEVEL", (-131072) as f64),
    ("ZSTD_btlazy2", (6) as f64),
    ("ZSTD_btopt", (7) as f64),
    ("ZSTD_btultra", (8) as f64),
    ("ZSTD_btultra2", (9) as f64),
    ("ZSTD_c_chainLog", (103) as f64),
    ("ZSTD_c_checksumFlag", (201) as f64),
    ("ZSTD_c_compressionLevel", (100) as f64),
    ("ZSTD_c_contentSizeFlag", (200) as f64),
    ("ZSTD_c_dictIDFlag", (202) as f64),
    ("ZSTD_c_enableLongDistanceMatching", (160) as f64),
    ("ZSTD_c_hashLog", (102) as f64),
    ("ZSTD_c_jobSize", (401) as f64),
    ("ZSTD_c_ldmBucketSizeLog", (163) as f64),
    ("ZSTD_c_ldmHashLog", (161) as f64),
    ("ZSTD_c_ldmHashRateLog", (164) as f64),
    ("ZSTD_c_ldmMinMatch", (162) as f64),
    ("ZSTD_c_minMatch", (105) as f64),
    ("ZSTD_c_nbWorkers", (400) as f64),
    ("ZSTD_c_overlapLog", (402) as f64),
    ("ZSTD_c_searchLog", (104) as f64),
    ("ZSTD_c_strategy", (107) as f64),
    ("ZSTD_c_targetLength", (106) as f64),
    ("ZSTD_c_windowLog", (101) as f64),
    ("ZSTD_d_windowLogMax", (100) as f64),
    ("ZSTD_dfast", (2) as f64),
    ("ZSTD_e_continue", (0) as f64),
    ("ZSTD_e_end", (2) as f64),
    ("ZSTD_e_flush", (1) as f64),
    ("ZSTD_error_GENERIC", (1) as f64),
    ("ZSTD_error_checksum_wrong", (22) as f64),
    ("ZSTD_error_corruption_detected", (20) as f64),
    ("ZSTD_error_dictionaryCreation_failed", (34) as f64),
    ("ZSTD_error_dictionary_corrupted", (30) as f64),
    ("ZSTD_error_dictionary_wrong", (32) as f64),
    ("ZSTD_error_dstBuffer_null", (74) as f64),
    ("ZSTD_error_dstSize_tooSmall", (70) as f64),
    ("ZSTD_error_frameParameter_unsupported", (14) as f64),
    ("ZSTD_error_frameParameter_windowTooLarge", (16) as f64),
    ("ZSTD_error_init_missing", (62) as f64),
    ("ZSTD_error_literals_headerWrong", (24) as f64),
    ("ZSTD_error_maxSymbolValue_tooLarge", (46) as f64),
    ("ZSTD_error_maxSymbolValue_tooSmall", (48) as f64),
    ("ZSTD_error_memory_allocation", (64) as f64),
    ("ZSTD_error_noForwardProgress_destFull", (80) as f64),
    ("ZSTD_error_noForwardProgress_inputEmpty", (82) as f64),
    ("ZSTD_error_no_error", (0) as f64),
    ("ZSTD_error_parameter_combination_unsupported", (41) as f64),
    ("ZSTD_error_parameter_outOfBound", (42) as f64),
    ("ZSTD_error_parameter_unsupported", (40) as f64),
    ("ZSTD_error_prefix_unknown", (10) as f64),
    ("ZSTD_error_srcSize_wrong", (72) as f64),
    ("ZSTD_error_stabilityCondition_notRespected", (50) as f64),
    ("ZSTD_error_stage_wrong", (60) as f64),
    ("ZSTD_error_tableLog_tooLarge", (44) as f64),
    ("ZSTD_error_version_unsupported", (12) as f64),
    ("ZSTD_error_workSpace_tooSmall", (66) as f64),
    ("ZSTD_fast", (1) as f64),
    ("ZSTD_greedy", (3) as f64),
    ("ZSTD_lazy", (4) as f64),
    ("ZSTD_lazy2", (5) as f64),
    ("Z_BEST_COMPRESSION", (9) as f64),
    ("Z_BEST_SPEED", (1) as f64),
    ("Z_BLOCK", (5) as f64),
    ("Z_BUF_ERROR", (-5) as f64),
    ("Z_DATA_ERROR", (-3) as f64),
    ("Z_DEFAULT_CHUNK", (16384) as f64),
    ("Z_DEFAULT_COMPRESSION", (-1) as f64),
    ("Z_DEFAULT_LEVEL", (-1) as f64),
    ("Z_DEFAULT_MEMLEVEL", (8) as f64),
    ("Z_DEFAULT_STRATEGY", (0) as f64),
    ("Z_DEFAULT_WINDOWBITS", (15) as f64),
    ("Z_ERRNO", (-1) as f64),
    ("Z_FILTERED", (1) as f64),
    ("Z_FINISH", (4) as f64),
    ("Z_FIXED", (4) as f64),
    ("Z_FULL_FLUSH", (3) as f64),
    ("Z_HUFFMAN_ONLY", (2) as f64),
    ("Z_MAX_CHUNK", (0x7fff_ffff) as f64),
    ("Z_MAX_LEVEL", (9) as f64),
    ("Z_MAX_MEMLEVEL", (9) as f64),
    ("Z_MAX_WINDOWBITS", (15) as f64),
    ("Z_MEM_ERROR", (-4) as f64),
    ("Z_MIN_CHUNK", (64) as f64),
    ("Z_MIN_LEVEL", (-1) as f64),
    ("Z_MIN_MEMLEVEL", (1) as f64),
    ("Z_MIN_WINDOWBITS", (8) as f64),
    ("Z_NEED_DICT", (2) as f64),
    ("Z_NO_COMPRESSION", (0) as f64),
    ("Z_NO_FLUSH", (0) as f64),
    ("Z_OK", (0) as f64),
    ("Z_PARTIAL_FLUSH", (1) as f64),
    ("Z_RLE", (3) as f64),
    ("Z_STREAM_END", (1) as f64),
    ("Z_STREAM_ERROR", (-2) as f64),
    ("Z_SYNC_FLUSH", (2) as f64),
    ("Z_VERSION_ERROR", (-6) as f64),
];

pub(super) fn zlib_const_lookup(prop: &str) -> Option<f64> {
    let i = ZLIB_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(ZLIB_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn zlib_const_reference(prop: &str) -> Option<f64> {
    let v: i64 = match prop {
        // Compression levels
        "Z_NO_COMPRESSION" => 0,
        "Z_BEST_SPEED" => 1,
        "Z_BEST_COMPRESSION" => 9,
        "Z_DEFAULT_COMPRESSION" => -1,
        // Compression strategies
        "Z_FILTERED" => 1,
        "Z_HUFFMAN_ONLY" => 2,
        "Z_RLE" => 3,
        "Z_FIXED" => 4,
        "Z_DEFAULT_STRATEGY" => 0,
        "ZLIB_VERNUM" => 0x1310,
        // Flush values
        "Z_NO_FLUSH" => 0,
        "Z_PARTIAL_FLUSH" => 1,
        "Z_SYNC_FLUSH" => 2,
        "Z_FULL_FLUSH" => 3,
        "Z_FINISH" => 4,
        "Z_BLOCK" => 5,
        // Return codes
        "Z_OK" => 0,
        "Z_STREAM_END" => 1,
        "Z_NEED_DICT" => 2,
        "Z_ERRNO" => -1,
        "Z_STREAM_ERROR" => -2,
        "Z_DATA_ERROR" => -3,
        "Z_MEM_ERROR" => -4,
        "Z_BUF_ERROR" => -5,
        "Z_VERSION_ERROR" => -6,
        // Min/Max window bits and memlevel
        "Z_MIN_WINDOWBITS" => 8,
        "Z_MAX_WINDOWBITS" => 15,
        "Z_DEFAULT_WINDOWBITS" => 15,
        "Z_MIN_CHUNK" => 64,
        "Z_MAX_CHUNK" => 0x7fff_ffff,
        "Z_DEFAULT_CHUNK" => 16384,
        "Z_MIN_MEMLEVEL" => 1,
        "Z_MAX_MEMLEVEL" => 9,
        "Z_DEFAULT_MEMLEVEL" => 8,
        "Z_MIN_LEVEL" => -1,
        "Z_MAX_LEVEL" => 9,
        "Z_DEFAULT_LEVEL" => -1,
        // Mode (zlib stream modes — used by zlib.createDeflate etc.)
        "DEFLATE" => 1,
        "INFLATE" => 2,
        "GZIP" => 3,
        "GUNZIP" => 4,
        "DEFLATERAW" => 5,
        "INFLATERAW" => 6,
        "UNZIP" => 7,
        "BROTLI_DECODE" => 8,
        "BROTLI_ENCODE" => 9,
        "ZSTD_COMPRESS" => 10,
        "ZSTD_DECOMPRESS" => 11,
        // Brotli operation/parameter constants — match Node's
        // `zlib.constants` exactly (these are the BrotliEncoder/
        // BrotliDecoder parameter ids the underlying brotli library
        // exposes).
        "BROTLI_OPERATION_PROCESS" => 0,
        "BROTLI_OPERATION_FLUSH" => 1,
        "BROTLI_OPERATION_FINISH" => 2,
        "BROTLI_OPERATION_EMIT_METADATA" => 3,
        "BROTLI_PARAM_MODE" => 0,
        "BROTLI_MODE_GENERIC" => 0,
        "BROTLI_MODE_TEXT" => 1,
        "BROTLI_MODE_FONT" => 2,
        "BROTLI_DEFAULT_MODE" => 0,
        "BROTLI_PARAM_QUALITY" => 1,
        "BROTLI_MIN_QUALITY" => 0,
        "BROTLI_MAX_QUALITY" => 11,
        "BROTLI_DEFAULT_QUALITY" => 11,
        "BROTLI_PARAM_LGWIN" => 2,
        "BROTLI_MIN_WINDOW_BITS" => 10,
        "BROTLI_MAX_WINDOW_BITS" => 24,
        "BROTLI_LARGE_MAX_WINDOW_BITS" => 30,
        "BROTLI_DEFAULT_WINDOW" => 22,
        "BROTLI_PARAM_LGBLOCK" => 3,
        "BROTLI_MIN_INPUT_BLOCK_BITS" => 16,
        "BROTLI_MAX_INPUT_BLOCK_BITS" => 24,
        "BROTLI_PARAM_DISABLE_LITERAL_CONTEXT_MODELING" => 4,
        "BROTLI_PARAM_SIZE_HINT" => 5,
        "BROTLI_PARAM_LARGE_WINDOW" => 6,
        "BROTLI_PARAM_NPOSTFIX" => 7,
        "BROTLI_PARAM_NDIRECT" => 8,
        "BROTLI_DECODER_RESULT_ERROR" => 0,
        "BROTLI_DECODER_RESULT_SUCCESS" => 1,
        "BROTLI_DECODER_RESULT_NEEDS_MORE_INPUT" => 2,
        "BROTLI_DECODER_RESULT_NEEDS_MORE_OUTPUT" => 3,
        "BROTLI_DECODER_PARAM_DISABLE_RING_BUFFER_REALLOCATION" => 0,
        "BROTLI_DECODER_PARAM_LARGE_WINDOW" => 1,
        // Zstd parameter ids — match Node's `zlib.constants`.
        "ZSTD_e_continue" => 0,
        "ZSTD_e_flush" => 1,
        "ZSTD_e_end" => 2,
        "ZSTD_fast" => 1,
        "ZSTD_dfast" => 2,
        "ZSTD_greedy" => 3,
        "ZSTD_lazy" => 4,
        "ZSTD_lazy2" => 5,
        "ZSTD_btlazy2" => 6,
        "ZSTD_btopt" => 7,
        "ZSTD_btultra" => 8,
        "ZSTD_btultra2" => 9,
        "ZSTD_c_compressionLevel" => 100,
        "ZSTD_c_windowLog" => 101,
        "ZSTD_c_hashLog" => 102,
        "ZSTD_c_chainLog" => 103,
        "ZSTD_c_searchLog" => 104,
        "ZSTD_c_minMatch" => 105,
        "ZSTD_c_targetLength" => 106,
        "ZSTD_c_strategy" => 107,
        "ZSTD_c_enableLongDistanceMatching" => 160,
        "ZSTD_c_ldmHashLog" => 161,
        "ZSTD_c_ldmMinMatch" => 162,
        "ZSTD_c_ldmBucketSizeLog" => 163,
        "ZSTD_c_ldmHashRateLog" => 164,
        "ZSTD_c_contentSizeFlag" => 200,
        "ZSTD_c_checksumFlag" => 201,
        "ZSTD_c_dictIDFlag" => 202,
        "ZSTD_c_nbWorkers" => 400,
        "ZSTD_c_jobSize" => 401,
        "ZSTD_c_overlapLog" => 402,
        "ZSTD_d_windowLogMax" => 100,
        "ZSTD_CLEVEL_DEFAULT" => 3,
        "ZSTD_MINCLEVEL" => -131072,
        "ZSTD_MAXCLEVEL" => 22,
        // #3677: Brotli decoder result/error codes Node exposes on
        // `zlib.constants` (the BrotliDecoderResult / BrotliDecoderErrorCode
        // enums). Required so `Object.keys(zlib.constants)` enumeration
        // matches Node's full set and every enumerated key reads its value.
        "BROTLI_DECODER_NO_ERROR" => 0,
        "BROTLI_DECODER_SUCCESS" => 1,
        "BROTLI_DECODER_NEEDS_MORE_INPUT" => 2,
        "BROTLI_DECODER_NEEDS_MORE_OUTPUT" => 3,
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_NIBBLE" => -1,
        "BROTLI_DECODER_ERROR_FORMAT_RESERVED" => -2,
        "BROTLI_DECODER_ERROR_FORMAT_EXUBERANT_META_NIBBLE" => -3,
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_ALPHABET" => -4,
        "BROTLI_DECODER_ERROR_FORMAT_SIMPLE_HUFFMAN_SAME" => -5,
        "BROTLI_DECODER_ERROR_FORMAT_CL_SPACE" => -6,
        "BROTLI_DECODER_ERROR_FORMAT_HUFFMAN_SPACE" => -7,
        "BROTLI_DECODER_ERROR_FORMAT_CONTEXT_MAP_REPEAT" => -8,
        "BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_1" => -9,
        "BROTLI_DECODER_ERROR_FORMAT_BLOCK_LENGTH_2" => -10,
        "BROTLI_DECODER_ERROR_FORMAT_TRANSFORM" => -11,
        "BROTLI_DECODER_ERROR_FORMAT_DICTIONARY" => -12,
        "BROTLI_DECODER_ERROR_FORMAT_WINDOW_BITS" => -13,
        "BROTLI_DECODER_ERROR_FORMAT_PADDING_1" => -14,
        "BROTLI_DECODER_ERROR_FORMAT_PADDING_2" => -15,
        "BROTLI_DECODER_ERROR_FORMAT_DISTANCE" => -16,
        "BROTLI_DECODER_ERROR_DICTIONARY_NOT_SET" => -19,
        "BROTLI_DECODER_ERROR_INVALID_ARGUMENTS" => -20,
        "BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MODES" => -21,
        "BROTLI_DECODER_ERROR_ALLOC_TREE_GROUPS" => -22,
        "BROTLI_DECODER_ERROR_ALLOC_CONTEXT_MAP" => -25,
        "BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_1" => -26,
        "BROTLI_DECODER_ERROR_ALLOC_RING_BUFFER_2" => -27,
        "BROTLI_DECODER_ERROR_ALLOC_BLOCK_TYPE_TREES" => -30,
        "BROTLI_DECODER_ERROR_UNREACHABLE" => -31,
        // #3677: Zstd error codes (ZSTD_ErrorCode enum) Node exposes.
        "ZSTD_error_no_error" => 0,
        "ZSTD_error_GENERIC" => 1,
        "ZSTD_error_prefix_unknown" => 10,
        "ZSTD_error_version_unsupported" => 12,
        "ZSTD_error_frameParameter_unsupported" => 14,
        "ZSTD_error_frameParameter_windowTooLarge" => 16,
        "ZSTD_error_corruption_detected" => 20,
        "ZSTD_error_checksum_wrong" => 22,
        "ZSTD_error_literals_headerWrong" => 24,
        "ZSTD_error_dictionary_corrupted" => 30,
        "ZSTD_error_dictionary_wrong" => 32,
        "ZSTD_error_dictionaryCreation_failed" => 34,
        "ZSTD_error_parameter_unsupported" => 40,
        "ZSTD_error_parameter_combination_unsupported" => 41,
        "ZSTD_error_parameter_outOfBound" => 42,
        "ZSTD_error_tableLog_tooLarge" => 44,
        "ZSTD_error_maxSymbolValue_tooLarge" => 46,
        "ZSTD_error_maxSymbolValue_tooSmall" => 48,
        "ZSTD_error_stabilityCondition_notRespected" => 50,
        "ZSTD_error_stage_wrong" => 60,
        "ZSTD_error_init_missing" => 62,
        "ZSTD_error_memory_allocation" => 64,
        "ZSTD_error_workSpace_tooSmall" => 66,
        "ZSTD_error_dstSize_tooSmall" => 70,
        "ZSTD_error_srcSize_wrong" => 72,
        "ZSTD_error_dstBuffer_null" => 74,
        "ZSTD_error_noForwardProgress_destFull" => 80,
        "ZSTD_error_noForwardProgress_inputEmpty" => 82,
        _ => return None,
    };
    Some(v as f64)
}

#[cfg(test)]
mod zlib_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in ZLIB_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = concat!(
            include_str!("constants.rs"),
            include_str!("constants_tables.rs")
        );
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    zlib_const_lookup(lit),
                    zlib_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `crypto_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
static CRYPTO_CONST_TABLE: &[(&str, f64)] = &[
    ("DH_CHECK_P_NOT_PRIME", (1.0) as f64),
    ("DH_CHECK_P_NOT_SAFE_PRIME", (2.0) as f64),
    ("DH_NOT_SUITABLE_GENERATOR", (8.0) as f64),
    ("DH_UNABLE_TO_CHECK_GENERATOR", (4.0) as f64),
    ("ENGINE_METHOD_ALL", (65535.0) as f64),
    ("ENGINE_METHOD_CIPHERS", (64.0) as f64),
    ("ENGINE_METHOD_DH", (4.0) as f64),
    ("ENGINE_METHOD_DIGESTS", (128.0) as f64),
    ("ENGINE_METHOD_DSA", (2.0) as f64),
    ("ENGINE_METHOD_EC", (2048.0) as f64),
    ("ENGINE_METHOD_NONE", (0.0) as f64),
    ("ENGINE_METHOD_PKEY_ASN1_METHS", (1024.0) as f64),
    ("ENGINE_METHOD_PKEY_METHS", (512.0) as f64),
    ("ENGINE_METHOD_RAND", (8.0) as f64),
    ("ENGINE_METHOD_RSA", (1.0) as f64),
    ("OPENSSL_VERSION_NUMBER", (811597840.0) as f64),
    ("POINT_CONVERSION_COMPRESSED", (2.0) as f64),
    ("POINT_CONVERSION_HYBRID", (6.0) as f64),
    ("POINT_CONVERSION_UNCOMPRESSED", (4.0) as f64),
    ("RSA_NO_PADDING", (3.0) as f64),
    ("RSA_PKCS1_OAEP_PADDING", (4.0) as f64),
    ("RSA_PKCS1_PADDING", (1.0) as f64),
    ("RSA_PKCS1_PSS_PADDING", (6.0) as f64),
    ("RSA_PSS_SALTLEN_AUTO", (-2.0) as f64),
    ("RSA_PSS_SALTLEN_DIGEST", (-1.0) as f64),
    ("RSA_PSS_SALTLEN_MAX_SIGN", (-2.0) as f64),
    ("RSA_X931_PADDING", (5.0) as f64),
    ("SSL_OP_ALL", (2147485776.0) as f64),
    ("SSL_OP_ALLOW_NO_DHE_KEX", (1024.0) as f64),
    (
        "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION",
        (262144.0) as f64,
    ),
    ("SSL_OP_CIPHER_SERVER_PREFERENCE", (4194304.0) as f64),
    ("SSL_OP_CISCO_ANYCONNECT", (32768.0) as f64),
    ("SSL_OP_COOKIE_EXCHANGE", (8192.0) as f64),
    ("SSL_OP_CRYPTOPRO_TLSEXT_BUG", (2147483648.0) as f64),
    ("SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS", (2048.0) as f64),
    ("SSL_OP_LEGACY_SERVER_CONNECT", (4.0) as f64),
    ("SSL_OP_NO_COMPRESSION", (131072.0) as f64),
    ("SSL_OP_NO_ENCRYPT_THEN_MAC", (524288.0) as f64),
    ("SSL_OP_NO_QUERY_MTU", (4096.0) as f64),
    ("SSL_OP_NO_RENEGOTIATION", (1073741824.0) as f64),
    (
        "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION",
        (65536.0) as f64,
    ),
    ("SSL_OP_NO_SSLv2", (0.0) as f64),
    ("SSL_OP_NO_SSLv3", (33554432.0) as f64),
    ("SSL_OP_NO_TICKET", (16384.0) as f64),
    ("SSL_OP_NO_TLSv1", (67108864.0) as f64),
    ("SSL_OP_NO_TLSv1_1", (268435456.0) as f64),
    ("SSL_OP_NO_TLSv1_2", (134217728.0) as f64),
    ("SSL_OP_NO_TLSv1_3", (536870912.0) as f64),
    ("SSL_OP_PRIORITIZE_CHACHA", (2097152.0) as f64),
    ("SSL_OP_TLS_ROLLBACK_BUG", (8388608.0) as f64),
    ("TLS1_1_VERSION", (770.0) as f64),
    ("TLS1_2_VERSION", (771.0) as f64),
    ("TLS1_3_VERSION", (772.0) as f64),
    ("TLS1_VERSION", (769.0) as f64),
];

pub(super) fn crypto_const_lookup(prop: &str) -> Option<f64> {
    let i = CRYPTO_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(CRYPTO_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn crypto_const_reference(prop: &str) -> Option<f64> {
    match prop {
        "OPENSSL_VERSION_NUMBER" => Some(811597840.0),
        "SSL_OP_ALL" => Some(2147485776.0),
        "SSL_OP_ALLOW_NO_DHE_KEX" => Some(1024.0),
        "SSL_OP_ALLOW_UNSAFE_LEGACY_RENEGOTIATION" => Some(262144.0),
        "SSL_OP_CIPHER_SERVER_PREFERENCE" => Some(4194304.0),
        "SSL_OP_CISCO_ANYCONNECT" => Some(32768.0),
        "SSL_OP_COOKIE_EXCHANGE" => Some(8192.0),
        "SSL_OP_CRYPTOPRO_TLSEXT_BUG" => Some(2147483648.0),
        "SSL_OP_DONT_INSERT_EMPTY_FRAGMENTS" => Some(2048.0),
        "SSL_OP_LEGACY_SERVER_CONNECT" => Some(4.0),
        "SSL_OP_NO_COMPRESSION" => Some(131072.0),
        "SSL_OP_NO_ENCRYPT_THEN_MAC" => Some(524288.0),
        "SSL_OP_NO_QUERY_MTU" => Some(4096.0),
        "SSL_OP_NO_RENEGOTIATION" => Some(1073741824.0),
        "SSL_OP_NO_SESSION_RESUMPTION_ON_RENEGOTIATION" => Some(65536.0),
        "SSL_OP_NO_SSLv2" => Some(0.0),
        "SSL_OP_NO_SSLv3" => Some(33554432.0),
        "SSL_OP_NO_TICKET" => Some(16384.0),
        "SSL_OP_NO_TLSv1" => Some(67108864.0),
        "SSL_OP_NO_TLSv1_1" => Some(268435456.0),
        "SSL_OP_NO_TLSv1_2" => Some(134217728.0),
        "SSL_OP_NO_TLSv1_3" => Some(536870912.0),
        "SSL_OP_PRIORITIZE_CHACHA" => Some(2097152.0),
        "SSL_OP_TLS_ROLLBACK_BUG" => Some(8388608.0),
        "ENGINE_METHOD_RSA" => Some(1.0),
        "ENGINE_METHOD_DSA" => Some(2.0),
        "ENGINE_METHOD_DH" => Some(4.0),
        "ENGINE_METHOD_RAND" => Some(8.0),
        "ENGINE_METHOD_EC" => Some(2048.0),
        "ENGINE_METHOD_CIPHERS" => Some(64.0),
        "ENGINE_METHOD_DIGESTS" => Some(128.0),
        "ENGINE_METHOD_PKEY_METHS" => Some(512.0),
        "ENGINE_METHOD_PKEY_ASN1_METHS" => Some(1024.0),
        "ENGINE_METHOD_ALL" => Some(65535.0),
        "ENGINE_METHOD_NONE" => Some(0.0),
        "DH_CHECK_P_NOT_SAFE_PRIME" => Some(2.0),
        "DH_CHECK_P_NOT_PRIME" => Some(1.0),
        "DH_UNABLE_TO_CHECK_GENERATOR" => Some(4.0),
        "DH_NOT_SUITABLE_GENERATOR" => Some(8.0),
        "RSA_PKCS1_PADDING" => Some(1.0),
        "RSA_NO_PADDING" => Some(3.0),
        "RSA_PKCS1_OAEP_PADDING" => Some(4.0),
        "RSA_X931_PADDING" => Some(5.0),
        "RSA_PKCS1_PSS_PADDING" => Some(6.0),
        "RSA_PSS_SALTLEN_DIGEST" => Some(-1.0),
        "RSA_PSS_SALTLEN_MAX_SIGN" => Some(-2.0),
        "RSA_PSS_SALTLEN_AUTO" => Some(-2.0),
        "TLS1_VERSION" => Some(769.0),
        "TLS1_1_VERSION" => Some(770.0),
        "TLS1_2_VERSION" => Some(771.0),
        "TLS1_3_VERSION" => Some(772.0),
        "POINT_CONVERSION_COMPRESSED" => Some(2.0),
        "POINT_CONVERSION_UNCOMPRESSED" => Some(4.0),
        "POINT_CONVERSION_HYBRID" => Some(6.0),
        _ => None,
    }
}

#[cfg(test)]
mod crypto_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in CRYPTO_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = concat!(
            include_str!("constants.rs"),
            include_str!("constants_tables.rs")
        );
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    crypto_const_lookup(lit),
                    crypto_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `os_errno_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
#[cfg(unix)]
static OS_ERRNO_CONST_TABLE: &[(&str, f64)] = &[
    ("E2BIG", (libc::E2BIG) as f64),
    ("EACCES", (libc::EACCES) as f64),
    ("EADDRINUSE", (libc::EADDRINUSE) as f64),
    ("EADDRNOTAVAIL", (libc::EADDRNOTAVAIL) as f64),
    ("EAFNOSUPPORT", (libc::EAFNOSUPPORT) as f64),
    ("EAGAIN", (libc::EAGAIN) as f64),
    ("EALREADY", (libc::EALREADY) as f64),
    ("EBADF", (libc::EBADF) as f64),
    ("EBADMSG", (libc::EBADMSG) as f64),
    ("EBUSY", (libc::EBUSY) as f64),
    ("ECANCELED", (libc::ECANCELED) as f64),
    ("ECHILD", (libc::ECHILD) as f64),
    ("ECONNABORTED", (libc::ECONNABORTED) as f64),
    ("ECONNREFUSED", (libc::ECONNREFUSED) as f64),
    ("ECONNRESET", (libc::ECONNRESET) as f64),
    ("EDEADLK", (libc::EDEADLK) as f64),
    ("EDESTADDRREQ", (libc::EDESTADDRREQ) as f64),
    ("EDOM", (libc::EDOM) as f64),
    ("EDQUOT", (libc::EDQUOT) as f64),
    ("EEXIST", (libc::EEXIST) as f64),
    ("EFAULT", (libc::EFAULT) as f64),
    ("EFBIG", (libc::EFBIG) as f64),
    ("EHOSTUNREACH", (libc::EHOSTUNREACH) as f64),
    ("EIDRM", (libc::EIDRM) as f64),
    ("EILSEQ", (libc::EILSEQ) as f64),
    ("EINPROGRESS", (libc::EINPROGRESS) as f64),
    ("EINTR", (libc::EINTR) as f64),
    ("EINVAL", (libc::EINVAL) as f64),
    ("EIO", (libc::EIO) as f64),
    ("EISCONN", (libc::EISCONN) as f64),
    ("EISDIR", (libc::EISDIR) as f64),
    ("ELOOP", (libc::ELOOP) as f64),
    ("EMFILE", (libc::EMFILE) as f64),
    ("EMLINK", (libc::EMLINK) as f64),
    ("EMSGSIZE", (libc::EMSGSIZE) as f64),
    ("EMULTIHOP", (libc::EMULTIHOP) as f64),
    ("ENAMETOOLONG", (libc::ENAMETOOLONG) as f64),
    ("ENETDOWN", (libc::ENETDOWN) as f64),
    ("ENETRESET", (libc::ENETRESET) as f64),
    ("ENETUNREACH", (libc::ENETUNREACH) as f64),
    ("ENFILE", (libc::ENFILE) as f64),
    ("ENOBUFS", (libc::ENOBUFS) as f64),
    ("ENODATA", (libc::ENODATA) as f64),
    ("ENODEV", (libc::ENODEV) as f64),
    ("ENOENT", (libc::ENOENT) as f64),
    ("ENOEXEC", (libc::ENOEXEC) as f64),
    ("ENOLCK", (libc::ENOLCK) as f64),
    ("ENOLINK", (libc::ENOLINK) as f64),
    ("ENOMEM", (libc::ENOMEM) as f64),
    ("ENOMSG", (libc::ENOMSG) as f64),
    ("ENOPROTOOPT", (libc::ENOPROTOOPT) as f64),
    ("ENOSPC", (libc::ENOSPC) as f64),
    ("ENOSR", (libc::ENOSR) as f64),
    ("ENOSTR", (libc::ENOSTR) as f64),
    ("ENOSYS", (libc::ENOSYS) as f64),
    ("ENOTCONN", (libc::ENOTCONN) as f64),
    ("ENOTDIR", (libc::ENOTDIR) as f64),
    ("ENOTEMPTY", (libc::ENOTEMPTY) as f64),
    ("ENOTSOCK", (libc::ENOTSOCK) as f64),
    ("ENOTSUP", (libc::ENOTSUP) as f64),
    ("ENOTTY", (libc::ENOTTY) as f64),
    ("ENXIO", (libc::ENXIO) as f64),
    ("EOPNOTSUPP", (libc::EOPNOTSUPP) as f64),
    ("EOVERFLOW", (libc::EOVERFLOW) as f64),
    ("EPERM", (libc::EPERM) as f64),
    ("EPIPE", (libc::EPIPE) as f64),
    ("EPROTO", (libc::EPROTO) as f64),
    ("EPROTONOSUPPORT", (libc::EPROTONOSUPPORT) as f64),
    ("EPROTOTYPE", (libc::EPROTOTYPE) as f64),
    ("ERANGE", (libc::ERANGE) as f64),
    ("EROFS", (libc::EROFS) as f64),
    ("ESPIPE", (libc::ESPIPE) as f64),
    ("ESRCH", (libc::ESRCH) as f64),
    ("ESTALE", (libc::ESTALE) as f64),
    ("ETIME", (libc::ETIME) as f64),
    ("ETIMEDOUT", (libc::ETIMEDOUT) as f64),
    ("ETXTBSY", (libc::ETXTBSY) as f64),
    ("EWOULDBLOCK", (libc::EWOULDBLOCK) as f64),
    ("EXDEV", (libc::EXDEV) as f64),
];
#[cfg(not(unix))]
static OS_ERRNO_CONST_TABLE: &[(&str, f64)] = &[
    ("EACCES", (13.0) as f64),
    ("EAGAIN", (11.0) as f64),
    ("EBADF", (9.0) as f64),
    ("EBUSY", (16.0) as f64),
    ("EEXIST", (17.0) as f64),
    ("EFAULT", (14.0) as f64),
    ("EINTR", (4.0) as f64),
    ("EINVAL", (22.0) as f64),
    ("EIO", (5.0) as f64),
    ("EISDIR", (21.0) as f64),
    ("EMFILE", (24.0) as f64),
    ("ENFILE", (23.0) as f64),
    ("ENODEV", (19.0) as f64),
    ("ENOENT", (2.0) as f64),
    ("ENOMEM", (12.0) as f64),
    ("ENOSPC", (28.0) as f64),
    ("ENOTDIR", (20.0) as f64),
    ("ENOTEMPTY", (41.0) as f64),
    ("EPERM", (1.0) as f64),
    ("EPIPE", (32.0) as f64),
    ("ERANGE", (34.0) as f64),
    ("EROFS", (30.0) as f64),
];

pub(super) fn os_errno_const_lookup(prop: &str) -> Option<f64> {
    let i = OS_ERRNO_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(OS_ERRNO_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn os_errno_const_reference(prop: &str) -> Option<f64> {
    #[cfg(unix)]
    {
        let v: Option<i32> = match prop {
            "E2BIG" => Some(libc::E2BIG),
            "EACCES" => Some(libc::EACCES),
            "EADDRINUSE" => Some(libc::EADDRINUSE),
            "EADDRNOTAVAIL" => Some(libc::EADDRNOTAVAIL),
            "EAFNOSUPPORT" => Some(libc::EAFNOSUPPORT),
            "EAGAIN" => Some(libc::EAGAIN),
            "EALREADY" => Some(libc::EALREADY),
            "EBADF" => Some(libc::EBADF),
            "EBADMSG" => Some(libc::EBADMSG),
            "EBUSY" => Some(libc::EBUSY),
            "ECANCELED" => Some(libc::ECANCELED),
            "ECHILD" => Some(libc::ECHILD),
            "ECONNABORTED" => Some(libc::ECONNABORTED),
            "ECONNREFUSED" => Some(libc::ECONNREFUSED),
            "ECONNRESET" => Some(libc::ECONNRESET),
            "EDEADLK" => Some(libc::EDEADLK),
            "EDESTADDRREQ" => Some(libc::EDESTADDRREQ),
            "EDOM" => Some(libc::EDOM),
            "EDQUOT" => Some(libc::EDQUOT),
            "EEXIST" => Some(libc::EEXIST),
            "EFAULT" => Some(libc::EFAULT),
            "EFBIG" => Some(libc::EFBIG),
            "EHOSTUNREACH" => Some(libc::EHOSTUNREACH),
            "EIDRM" => Some(libc::EIDRM),
            "EILSEQ" => Some(libc::EILSEQ),
            "EINPROGRESS" => Some(libc::EINPROGRESS),
            "EINTR" => Some(libc::EINTR),
            "EINVAL" => Some(libc::EINVAL),
            "EIO" => Some(libc::EIO),
            "EISCONN" => Some(libc::EISCONN),
            "EISDIR" => Some(libc::EISDIR),
            "ELOOP" => Some(libc::ELOOP),
            "EMFILE" => Some(libc::EMFILE),
            "EMLINK" => Some(libc::EMLINK),
            "EMSGSIZE" => Some(libc::EMSGSIZE),
            "EMULTIHOP" => Some(libc::EMULTIHOP),
            "ENAMETOOLONG" => Some(libc::ENAMETOOLONG),
            "ENETDOWN" => Some(libc::ENETDOWN),
            "ENETRESET" => Some(libc::ENETRESET),
            "ENETUNREACH" => Some(libc::ENETUNREACH),
            "ENFILE" => Some(libc::ENFILE),
            "ENOBUFS" => Some(libc::ENOBUFS),
            "ENODATA" => Some(libc::ENODATA),
            "ENODEV" => Some(libc::ENODEV),
            "ENOENT" => Some(libc::ENOENT),
            "ENOEXEC" => Some(libc::ENOEXEC),
            "ENOLCK" => Some(libc::ENOLCK),
            "ENOLINK" => Some(libc::ENOLINK),
            "ENOMEM" => Some(libc::ENOMEM),
            "ENOMSG" => Some(libc::ENOMSG),
            "ENOPROTOOPT" => Some(libc::ENOPROTOOPT),
            "ENOSPC" => Some(libc::ENOSPC),
            "ENOSR" => Some(libc::ENOSR),
            "ENOSTR" => Some(libc::ENOSTR),
            "ENOSYS" => Some(libc::ENOSYS),
            "ENOTCONN" => Some(libc::ENOTCONN),
            "ENOTDIR" => Some(libc::ENOTDIR),
            "ENOTEMPTY" => Some(libc::ENOTEMPTY),
            "ENOTSOCK" => Some(libc::ENOTSOCK),
            "ENOTSUP" => Some(libc::ENOTSUP),
            "ENOTTY" => Some(libc::ENOTTY),
            "ENXIO" => Some(libc::ENXIO),
            "EOPNOTSUPP" => Some(libc::EOPNOTSUPP),
            "EOVERFLOW" => Some(libc::EOVERFLOW),
            "EPERM" => Some(libc::EPERM),
            "EPIPE" => Some(libc::EPIPE),
            "EPROTO" => Some(libc::EPROTO),
            "EPROTONOSUPPORT" => Some(libc::EPROTONOSUPPORT),
            "EPROTOTYPE" => Some(libc::EPROTOTYPE),
            "ERANGE" => Some(libc::ERANGE),
            "EROFS" => Some(libc::EROFS),
            "ESPIPE" => Some(libc::ESPIPE),
            "ESRCH" => Some(libc::ESRCH),
            "ESTALE" => Some(libc::ESTALE),
            "ETIME" => Some(libc::ETIME),
            "ETIMEDOUT" => Some(libc::ETIMEDOUT),
            "ETXTBSY" => Some(libc::ETXTBSY),
            "EWOULDBLOCK" => Some(libc::EWOULDBLOCK),
            "EXDEV" => Some(libc::EXDEV),
            _ => None,
        };
        v.map(|x| x as f64)
    }
    #[cfg(not(unix))]
    {
        match prop {
            "EACCES" => Some(13.0),
            "EAGAIN" => Some(11.0),
            "EBADF" => Some(9.0),
            "EBUSY" => Some(16.0),
            "EEXIST" => Some(17.0),
            "EFAULT" => Some(14.0),
            "EINTR" => Some(4.0),
            "EINVAL" => Some(22.0),
            "EIO" => Some(5.0),
            "EISDIR" => Some(21.0),
            "EMFILE" => Some(24.0),
            "ENFILE" => Some(23.0),
            "ENODEV" => Some(19.0),
            "ENOENT" => Some(2.0),
            "ENOMEM" => Some(12.0),
            "ENOSPC" => Some(28.0),
            "ENOTDIR" => Some(20.0),
            "ENOTEMPTY" => Some(41.0),
            "EPERM" => Some(1.0),
            "EPIPE" => Some(32.0),
            "ERANGE" => Some(34.0),
            "EROFS" => Some(30.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod os_errno_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in OS_ERRNO_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = concat!(
            include_str!("constants.rs"),
            include_str!("constants_tables.rs")
        );
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    os_errno_const_lookup(lit),
                    os_errno_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}

/// Sorted table for `os_signal_const`: block-level `#[cfg]` predicates mirrored
/// verbatim as per-platform tables; per-arm cfg chains composed with
/// `not(any(..))` so first-match-wins semantics survive; every value kept
/// as its verbatim const expression cast to f64 (uniform row type across
/// platforms). The verbatim reference + literal-universe oracle below runs
/// on EVERY platform in CI, covering the sides not testable locally.
#[cfg(unix)]
static OS_SIGNAL_CONST_TABLE: &[(&str, f64)] = &[
    ("SIGABRT", (libc::SIGABRT) as f64),
    ("SIGALRM", (libc::SIGALRM) as f64),
    ("SIGBUS", (libc::SIGBUS) as f64),
    ("SIGCHLD", (libc::SIGCHLD) as f64),
    ("SIGCONT", (libc::SIGCONT) as f64),
    ("SIGFPE", (libc::SIGFPE) as f64),
    ("SIGHUP", (libc::SIGHUP) as f64),
    ("SIGILL", (libc::SIGILL) as f64),
    #[cfg(target_os = "macos")]
    ("SIGINFO", (29i32) as f64),
    ("SIGINT", (libc::SIGINT) as f64),
    ("SIGIO", (libc::SIGIO) as f64),
    ("SIGIOT", (libc::SIGABRT) as f64),
    ("SIGKILL", (libc::SIGKILL) as f64),
    ("SIGPIPE", (libc::SIGPIPE) as f64),
    #[cfg(any(target_os = "linux", target_os = "android"))]
    ("SIGPOLL", (libc::SIGPOLL) as f64),
    ("SIGPROF", (libc::SIGPROF) as f64),
    #[cfg(target_os = "linux")]
    ("SIGPWR", (libc::SIGPWR) as f64),
    ("SIGQUIT", (libc::SIGQUIT) as f64),
    ("SIGSEGV", (libc::SIGSEGV) as f64),
    #[cfg(target_os = "linux")]
    ("SIGSTKFLT", (libc::SIGSTKFLT) as f64),
    ("SIGSTOP", (libc::SIGSTOP) as f64),
    ("SIGSYS", (libc::SIGSYS) as f64),
    ("SIGTERM", (libc::SIGTERM) as f64),
    ("SIGTRAP", (libc::SIGTRAP) as f64),
    ("SIGTSTP", (libc::SIGTSTP) as f64),
    ("SIGTTIN", (libc::SIGTTIN) as f64),
    ("SIGTTOU", (libc::SIGTTOU) as f64),
    ("SIGURG", (libc::SIGURG) as f64),
    ("SIGUSR1", (libc::SIGUSR1) as f64),
    ("SIGUSR2", (libc::SIGUSR2) as f64),
    ("SIGVTALRM", (libc::SIGVTALRM) as f64),
    ("SIGWINCH", (libc::SIGWINCH) as f64),
    ("SIGXCPU", (libc::SIGXCPU) as f64),
    ("SIGXFSZ", (libc::SIGXFSZ) as f64),
];
#[cfg(not(unix))]
static OS_SIGNAL_CONST_TABLE: &[(&str, f64)] = &[
    ("SIGABRT", (22.0) as f64),
    ("SIGBREAK", (21.0) as f64),
    ("SIGFPE", (8.0) as f64),
    ("SIGHUP", (1.0) as f64),
    ("SIGILL", (4.0) as f64),
    ("SIGINT", (2.0) as f64),
    ("SIGKILL", (9.0) as f64),
    ("SIGSEGV", (11.0) as f64),
    ("SIGTERM", (15.0) as f64),
];

pub(super) fn os_signal_const_lookup(prop: &str) -> Option<f64> {
    let i = OS_SIGNAL_CONST_TABLE
        .binary_search_by(|(n, _)| (*n).cmp(prop))
        .ok()?;
    Some(OS_SIGNAL_CONST_TABLE[i].1)
}

#[cfg(test)]
#[allow(clippy::unnecessary_cast)]
fn os_signal_const_reference(prop: &str) -> Option<f64> {
    #[cfg(unix)]
    {
        let v: Option<i32> = match prop {
            "SIGHUP" => Some(libc::SIGHUP),
            "SIGINT" => Some(libc::SIGINT),
            "SIGQUIT" => Some(libc::SIGQUIT),
            "SIGILL" => Some(libc::SIGILL),
            "SIGTRAP" => Some(libc::SIGTRAP),
            "SIGABRT" => Some(libc::SIGABRT),
            "SIGIOT" => Some(libc::SIGABRT),
            "SIGBUS" => Some(libc::SIGBUS),
            "SIGFPE" => Some(libc::SIGFPE),
            "SIGKILL" => Some(libc::SIGKILL),
            "SIGUSR1" => Some(libc::SIGUSR1),
            "SIGSEGV" => Some(libc::SIGSEGV),
            "SIGUSR2" => Some(libc::SIGUSR2),
            "SIGPIPE" => Some(libc::SIGPIPE),
            "SIGALRM" => Some(libc::SIGALRM),
            "SIGTERM" => Some(libc::SIGTERM),
            "SIGCHLD" => Some(libc::SIGCHLD),
            #[cfg(target_os = "linux")]
            "SIGSTKFLT" => Some(libc::SIGSTKFLT),
            "SIGCONT" => Some(libc::SIGCONT),
            "SIGSTOP" => Some(libc::SIGSTOP),
            "SIGTSTP" => Some(libc::SIGTSTP),
            "SIGTTIN" => Some(libc::SIGTTIN),
            "SIGTTOU" => Some(libc::SIGTTOU),
            "SIGURG" => Some(libc::SIGURG),
            "SIGXCPU" => Some(libc::SIGXCPU),
            "SIGXFSZ" => Some(libc::SIGXFSZ),
            "SIGVTALRM" => Some(libc::SIGVTALRM),
            "SIGPROF" => Some(libc::SIGPROF),
            "SIGWINCH" => Some(libc::SIGWINCH),
            "SIGIO" => Some(libc::SIGIO),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            "SIGPOLL" => Some(libc::SIGPOLL),
            #[cfg(target_os = "linux")]
            "SIGPWR" => Some(libc::SIGPWR),
            "SIGSYS" => Some(libc::SIGSYS),
            #[cfg(target_os = "macos")]
            "SIGINFO" => Some(29i32),
            _ => None,
        };
        v.map(|x| x as f64)
    }
    #[cfg(not(unix))]
    {
        match prop {
            "SIGHUP" => Some(1.0),
            "SIGINT" => Some(2.0),
            "SIGILL" => Some(4.0),
            "SIGABRT" => Some(22.0),
            "SIGFPE" => Some(8.0),
            "SIGKILL" => Some(9.0),
            "SIGSEGV" => Some(11.0),
            "SIGTERM" => Some(15.0),
            "SIGBREAK" => Some(21.0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod os_signal_const_table_tests {
    use super::*;
    #[test]
    fn sorted() {
        for w in OS_SIGNAL_CONST_TABLE.windows(2) {
            assert!(w[0].0 < w[1].0, "{} vs {}", w[0].0, w[1].0);
        }
    }
    #[test]
    fn matches_reference_exhaustively() {
        let source = concat!(
            include_str!("constants.rs"),
            include_str!("constants_tables.rs")
        );
        let mut rest = source;
        while let Some(s) = rest.find('"') {
            let after = &rest[s + 1..];
            let Some(e) = after.find('"') else { break };
            let lit = &after[..e];
            if !lit.is_empty() && lit.len() < 64 {
                assert_eq!(
                    os_signal_const_lookup(lit),
                    os_signal_const_reference(lit),
                    "at {lit}"
                );
            }
            rest = &after[e + 1..];
        }
    }
}
