//! The last 127 prefix positions cannot be seeded into H10's 128-byte tree.
//! Search them separately without imposing an order on unavailable future bytes.
use core::cmp::min;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CustomPrefix {
    end: Option<usize>,
}

impl CustomPrefix {
    pub(crate) fn new(end: usize) -> Self {
        Self {
            end: if end > 1 { Some(end) } else { None },
        }
    }

    /// Use the encoder's unwrapped position so expired metadata cannot become
    /// live again when WrapPosition reuses a smaller integer position.
    pub(crate) fn expire_before(&mut self, position: u64, max_backward: usize) {
        if let Some(end) = self.end {
            if position.saturating_sub(end as u64) >= max_backward as u64 {
                self.end = None;
            }
        }
    }

    pub(crate) fn find_matches<F: FnMut(usize, usize)>(
        &mut self,
        data: &[u8],
        mask: usize,
        position: usize,
        max_length: usize,
        max_backward: usize,
        mut found: F,
    ) {
        let end = match self.end {
            Some(end) if position >= end => end,
            Some(_) => {
                self.end = None;
                return;
            }
            None => return,
        };
        self.expire_before(position as u64, max_backward);
        if self.end.is_none() {
            return;
        }
        let current = position & mask;
        if current >= data.len() {
            return;
        }
        // Prefix bytes are addressed before the first ring wrap. Their history
        // expires within one window; the encoder's ring holds at least two.
        for previous in (end.saturating_sub(127)..end).rev() {
            let distance = position - previous;
            if distance > max_backward {
                break;
            }
            let source = previous & mask;
            if source >= data.len() {
                continue;
            }
            // Never read across the dictionary break, the available input, or
            // either physical slice. In particular no padded lookahead is read.
            let limit = min(
                min(end - previous, max_length),
                min(data.len() - source, data.len() - current),
            );
            if limit < 2 || data[source] != data[current] || data[source + 1] != data[current + 1] {
                continue;
            }
            let mut length = 2;
            while length < limit && data[source + length] == data[current + length] {
                length += 1;
            }
            found(distance, length);
        }
    }
}
