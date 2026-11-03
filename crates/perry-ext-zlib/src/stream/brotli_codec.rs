//! Owned-byte Brotli operations: no JS pointers survive codec work.
use brotli::enc::{BrotliEncoderParams, StandardAlloc};
use brotli::{Allocator, SliceWrapperMut};

pub(super) fn compress(
    data: &[u8],
    dictionary: Option<&[u8]>,
    quality: i32,
) -> std::io::Result<Vec<u8>> {
    let params = BrotliEncoderParams {
        quality,
        lgwin: 22,
        ..Default::default()
    };
    let mut input = data;
    let mut output = Vec::new();
    brotli::BrotliCompressCustomIoCustomDict(
        &mut brotli::IoReaderWrapper(&mut input),
        &mut brotli::IoWriterWrapper(&mut output),
        &mut [0; 4096],
        &mut [0; 4096],
        &params,
        StandardAlloc::default(),
        &mut |_, _, _, _| (),
        dictionary.unwrap_or_default(),
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Unexpected EOF"),
    )?;
    Ok(output)
}

#[derive(Debug)]
pub(super) enum DecodeError {
    Decode(String),
    OutputLimit,
}

pub(super) fn decompress(
    data: &[u8],
    dictionary: Option<Vec<u8>>,
    limit: usize,
) -> Result<Vec<u8>, DecodeError> {
    let dictionary = dictionary.unwrap_or_default();
    let mut alloc = StandardAlloc::default();
    let mut custom_dictionary = alloc.alloc_cell(dictionary.len());
    custom_dictionary.slice_mut().copy_from_slice(&dictionary);
    let mut state = brotli::BrotliState::new_with_custom_dictionary(
        alloc,
        StandardAlloc::default(),
        StandardAlloc::default(),
        custom_dictionary,
    );
    let mut available_in = data.len();
    let mut input_offset = 0;
    let mut total_out = 0;
    let mut output = Vec::new();
    let mut scratch = [0; 4096];
    loop {
        let remaining = limit.saturating_sub(output.len());
        // One lookahead byte distinguishes exact-limit EOF from overflow.
        let capacity = remaining.saturating_add(1).min(scratch.len());
        let mut available_out = capacity;
        let mut output_offset = 0;
        let result = brotli::BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            data,
            &mut available_out,
            &mut output_offset,
            &mut scratch[..capacity],
            &mut total_out,
            &mut state,
        );
        if output_offset > remaining {
            return Err(DecodeError::OutputLimit);
        }
        output.extend_from_slice(&scratch[..output_offset]);
        match result {
            brotli::BrotliResult::ResultSuccess => return Ok(output),
            brotli::BrotliResult::NeedsMoreOutput => (),
            brotli::BrotliResult::ResultFailure => {
                let error = format!("{:?}", state.error_code);
                let name = error.strip_prefix("BROTLI_DECODER_").unwrap_or(&error);
                return Err(DecodeError::Decode(format!("ERR__{name}")));
            }
            brotli::BrotliResult::NeedsMoreInput => {
                // Preserve the existing truncated-input fallback.
                return Err(DecodeError::Decode("ERR__ERROR_FORMAT_PADDING_2".into()));
            }
        }
    }
}
