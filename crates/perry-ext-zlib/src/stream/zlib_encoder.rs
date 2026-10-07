//! The deflate-family stream encoder (gzip, zlib, raw) on the same pinned
//! miniz_oxide backend that the one-shot codecs use through flate2. The
//! compressor always emits raw deflate; this payload owns the wrapper: the
//! gzip header with its CRC-32/size trailer and the zlib header with its
//! Adler-32 trailer. Header and trailer bytes wait in a fixed buffer until
//! the runtime supplies output space, so no output queue is retained.
use super::*;
use miniz_oxide::deflate::core::{CompressionStrategy, CompressorOxide};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

enum Check {
    Gzip { crc: flate2::Crc },
    Zlib { adler: adler2::Adler32 },
    Raw,
}

pub(super) struct Encoder {
    compressor: Box<CompressorOxide>,
    check: Check,
    /// Wrapper bytes (a header or a trailer) not yet copied to output.
    pending: [u8; 10],
    pending_start: u8,
    pending_end: u8,
    finished: bool,
}

fn strategy_of(strategy: i32) -> CompressionStrategy {
    match strategy {
        1 => CompressionStrategy::Filtered,
        2 => CompressionStrategy::HuffmanOnly,
        3 => CompressionStrategy::RLE,
        4 => CompressionStrategy::Fixed,
        _ => CompressionStrategy::Default,
    }
}

fn raw_compressor(level: Compression, strategy: i32) -> Box<CompressorOxide> {
    Box::new(CompressorOxide::with_params(
        DataFormat::Raw,
        level.level().min(9) as u8,
        strategy_of(strategy),
        15,
    ))
}

impl Encoder {
    pub(super) fn new(codec: Codec, level: Compression) -> std::io::Result<Self> {
        let level_value = level.level().min(9);
        let mut pending = [0; 10];
        let (check, header_len) = match codec {
            Codec::Gzip => {
                let os = if cfg!(target_os = "macos") {
                    19
                } else if cfg!(target_os = "windows") {
                    10
                } else {
                    3
                };
                // XFL as zlib writes it: 2 for level 9, 4 for levels 0 and 1.
                let xfl = if level_value == 9 {
                    2
                } else if level_value < 2 {
                    4
                } else {
                    0
                };
                pending = [0x1f, 0x8b, 8, 0, 0, 0, 0, 0, xfl, os];
                (
                    Check::Gzip {
                        crc: flate2::Crc::new(),
                    },
                    10,
                )
            }
            Codec::Deflate => {
                let level_flags: u16 = match level_value {
                    0 | 1 => 0,
                    2..=5 => 1,
                    6 => 2,
                    _ => 3,
                };
                let mut header: u16 = (0x78 << 8) | (level_flags << 6);
                header += 31 - header % 31;
                pending[..2].copy_from_slice(&header.to_be_bytes());
                (
                    Check::Zlib {
                        adler: adler2::Adler32::new(),
                    },
                    2,
                )
            }
            Codec::DeflateRaw => (Check::Raw, 0),
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "not a deflate-family encoder",
                ))
            }
        };
        Ok(Self {
            compressor: raw_compressor(level, 0),
            check,
            pending,
            pending_start: 0,
            pending_end: header_len,
            finished: false,
        })
    }

    pub(super) fn native_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + allocation::deflate_bytes()
    }

    fn drain_pending(&mut self, output: &mut [u8]) -> usize {
        let start = self.pending_start as usize;
        let n = (self.pending_end as usize - start).min(output.len());
        output[..n].copy_from_slice(&self.pending[start..start + n]);
        self.pending_start += n as u8;
        n
    }

    fn queue_trailer(&mut self) {
        let mut trailer = [0; 10];
        let len = match &self.check {
            Check::Gzip { crc } => {
                trailer[..4].copy_from_slice(&crc.sum().to_le_bytes());
                trailer[4..8].copy_from_slice(&crc.amount().to_le_bytes());
                8
            }
            Check::Zlib { adler } => {
                trailer[..4].copy_from_slice(&adler.checksum().to_be_bytes());
                4
            }
            Check::Raw => 0,
        };
        self.pending = trailer;
        self.pending_start = 0;
        self.pending_end = len;
    }

    pub(super) fn step(
        &mut self,
        op: &ns::StepIn,
        input: &[u8],
        output: &mut [u8],
    ) -> std::io::Result<(usize, usize, ns::StepStatus)> {
        let final_op = op.op == ns::StreamOp::FINAL;
        let mut written = self.drain_pending(output);
        if self.pending_start < self.pending_end {
            return Ok((0, written, ns::StepStatus::MORE));
        }
        if self.finished {
            let status = if final_op {
                ns::StepStatus::ENDED
            } else {
                ns::StepStatus::NEED_INPUT
            };
            return Ok((0, written, status));
        }
        if written == output.len() {
            return Ok((0, written, ns::StepStatus::MORE));
        }
        let flush = if final_op {
            MZFlush::Finish
        } else if op.op == ns::StreamOp::FLUSH {
            match op.flush_kind {
                0 => MZFlush::None,
                1 => MZFlush::Partial,
                3 => MZFlush::Full,
                4 => MZFlush::Finish,
                // Z_SYNC_FLUSH, and Z_BLOCK, which this backend completes as
                // a sync flush.
                _ => MZFlush::Sync,
            }
        } else {
            MZFlush::None
        };
        let result = miniz_oxide::deflate::stream::deflate(
            &mut self.compressor,
            input,
            &mut output[written..],
            flush,
        );
        let consumed = result.bytes_consumed;
        match &mut self.check {
            Check::Gzip { crc } => crc.update(&input[..consumed]),
            Check::Zlib { adler } => adler.write_slice(&input[..consumed]),
            Check::Raw => {}
        }
        written += result.bytes_written;
        let status = match result.status {
            Ok(MZStatus::StreamEnd) => {
                self.finished = true;
                self.queue_trailer();
                written += self.drain_pending(&mut output[written..]);
                if self.pending_start < self.pending_end {
                    ns::StepStatus::MORE
                } else if final_op {
                    ns::StepStatus::ENDED
                } else {
                    ns::StepStatus::NEED_INPUT
                }
            }
            Ok(_) | Err(MZError::Buf) => {
                if consumed < input.len() || written == output.len() || final_op {
                    ns::StepStatus::MORE
                } else {
                    ns::StepStatus::NEED_INPUT
                }
            }
            Err(error) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("deflate: {error:?}"),
                ))
            }
        };
        Ok((consumed, written, status))
    }

    /// Change level and strategy for the rest of the stream. The runtime runs
    /// a sync flush first, so every earlier byte already ends on a block
    /// boundary; later blocks come from a fresh compressor while the
    /// wrapper's running check continues.
    pub(super) fn params(&mut self, level: Compression, strategy: i32) {
        debug_assert_eq!(self.pending_start, self.pending_end);
        if !self.finished {
            self.compressor = raw_compressor(level, strategy);
        }
    }
}
