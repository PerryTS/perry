//! RFC 7541 HPACK, implemented here. Bounded dynamic table and decoded output.
use crate::{Error, Result, http1::Header};
#[path = "huffman_table.rs"]
mod huffman_table;
use huffman_table::CODES;
const STATIC: [(&str, &str); 61] = [
    (":authority", ""),
    (":method", "GET"),
    (":method", "POST"),
    (":path", "/"),
    (":path", "/index.html"),
    (":scheme", "http"),
    (":scheme", "https"),
    (":status", "200"),
    (":status", "204"),
    (":status", "206"),
    (":status", "304"),
    (":status", "400"),
    (":status", "404"),
    (":status", "500"),
    ("accept-charset", ""),
    ("accept-encoding", "gzip, deflate"),
    ("accept-language", ""),
    ("accept-ranges", ""),
    ("accept", ""),
    ("access-control-allow-origin", ""),
    ("age", ""),
    ("allow", ""),
    ("authorization", ""),
    ("cache-control", ""),
    ("content-disposition", ""),
    ("content-encoding", ""),
    ("content-language", ""),
    ("content-length", ""),
    ("content-location", ""),
    ("content-range", ""),
    ("content-type", ""),
    ("cookie", ""),
    ("date", ""),
    ("etag", ""),
    ("expect", ""),
    ("expires", ""),
    ("from", ""),
    ("host", ""),
    ("if-match", ""),
    ("if-modified-since", ""),
    ("if-none-match", ""),
    ("if-range", ""),
    ("if-unmodified-since", ""),
    ("last-modified", ""),
    ("link", ""),
    ("location", ""),
    ("max-forwards", ""),
    ("proxy-authenticate", ""),
    ("proxy-authorization", ""),
    ("range", ""),
    ("referer", ""),
    ("refresh", ""),
    ("retry-after", ""),
    ("server", ""),
    ("set-cookie", ""),
    ("strict-transport-security", ""),
    ("transfer-encoding", ""),
    ("user-agent", ""),
    ("vary", ""),
    ("via", ""),
    ("www-authenticate", ""),
];
fn bad() -> Error {
    Error::new("COMPRESSION_ERROR", "invalid HPACK block")
}
pub fn encode_integer(mut value: usize, bits: u8, prefix: u8, out: &mut Vec<u8>) {
    let mask = (1usize << bits) - 1;
    if value < mask {
        out.push(prefix | value as u8);
        return;
    }
    out.push(prefix | mask as u8);
    value -= mask;
    while value >= 128 {
        out.push((value as u8 & 127) | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn integer(input: &[u8], pos: &mut usize, bits: u8) -> Result<usize> {
    let mask = (1usize << bits) - 1;
    let mut n = (*input.get(*pos).ok_or_else(bad)? as usize) & mask;
    *pos += 1;
    if n < mask {
        return Ok(n);
    }
    let mut shift = 0;
    loop {
        let b = *input.get(*pos).ok_or_else(bad)?;
        *pos += 1;
        if shift >= usize::BITS || (b as usize & 127) > (usize::MAX >> shift) {
            return Err(bad());
        }
        n = n.checked_add((b as usize & 127) << shift).ok_or_else(bad)?;
        if b & 128 == 0 {
            return Ok(n);
        }
        shift += 7;
    }
}
pub fn huffman_encode(input: &[u8], out: &mut Vec<u8>) {
    let mut acc = 0u64;
    let mut bits = 0u8;
    for b in input {
        let (code, len) = CODES[*b as usize];
        acc = (acc << len) | code as u64;
        bits += len;
        while bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
        acc &= (1u64 << bits) - 1;
    }
    if bits > 0 {
        out.push(((acc << (8 - bits)) | ((1 << (8 - bits)) - 1)) as u8);
    }
}
#[derive(Clone, Copy)]
struct Node {
    child: [usize; 2],
    symbol: Option<u16>,
}
fn trie() -> &'static [Node] {
    static TREE: std::sync::OnceLock<Vec<Node>> = std::sync::OnceLock::new();
    TREE.get_or_init(|| {
        let mut nodes = vec![Node {
            child: [0; 2],
            symbol: None,
        }];
        for (symbol, (code, len)) in CODES.iter().enumerate() {
            let mut node = 0;
            for shift in (0..*len).rev() {
                let bit = ((code >> shift) & 1) as usize;
                if nodes[node].child[bit] == 0 {
                    nodes[node].child[bit] = nodes.len();
                    nodes.push(Node {
                        child: [0; 2],
                        symbol: None,
                    });
                }
                node = nodes[node].child[bit];
            }
            nodes[node].symbol = Some(symbol as u16);
        }
        nodes
    })
}
pub fn huffman_decode(input: &[u8], out: &mut Vec<u8>, limit: usize) -> Result<()> {
    let nodes = trie();
    let mut node = 0;
    let mut pending = 0;
    let mut ones = true;
    for b in input {
        for bit in (0..8).rev() {
            let value = ((b >> bit) & 1) as usize;
            node = nodes[node].child[value];
            if node == 0 {
                return Err(bad());
            }
            pending += 1;
            ones &= value == 1;
            if let Some(symbol) = nodes[node].symbol {
                if symbol == 256 || out.len() >= limit {
                    return Err(bad());
                }
                out.push(symbol as u8);
                node = 0;
                pending = 0;
                ones = true;
            }
        }
    }
    if pending > 7 || !ones {
        return Err(bad());
    }
    Ok(())
}
fn string(input: &[u8], pos: &mut usize, limit: usize) -> Result<Vec<u8>> {
    let huffman = *input.get(*pos).ok_or_else(bad)? & 128 != 0;
    let n = integer(input, pos, 7)?;
    let end = pos.checked_add(n).ok_or_else(bad)?;
    let raw = input.get(*pos..end).ok_or_else(bad)?;
    *pos = end;
    let mut out = Vec::new();
    if huffman {
        huffman_decode(raw, &mut out, limit)?;
    } else {
        if n > limit {
            return Err(bad());
        }
        out.extend_from_slice(raw);
    }
    Ok(out)
}
fn emit_string(value: &[u8], out: &mut Vec<u8>) {
    let bits: usize = value.iter().map(|b| CODES[*b as usize].1 as usize).sum();
    let n = bits.div_ceil(8);
    if n < value.len() {
        encode_integer(n, 7, 128, out);
        huffman_encode(value, out);
    } else {
        encode_integer(value.len(), 7, 0, out);
        out.extend_from_slice(value);
    }
}
#[derive(Debug)]
struct Table {
    slots: Vec<Header>,
    used: usize,
    bytes: usize,
    max: usize,
}
impl Table {
    fn new(max: usize) -> Self {
        Self {
            slots: Vec::new(),
            used: 0,
            bytes: 0,
            max,
        }
    }
    fn resize(&mut self, max: usize) {
        self.max = max;
        while self.bytes > max {
            self.evict();
        }
    }
    fn evict(&mut self) {
        self.used -= 1;
        let h = &self.slots[self.used];
        self.bytes -= h.name.len() + h.value.len() + 32;
    }
    fn add(&mut self, name: &str, value: &[u8]) {
        let size = name.len() + value.len() + 32;
        if size > self.max {
            self.used = 0;
            self.bytes = 0;
            return;
        }
        while self.bytes + size > self.max {
            self.evict();
        }
        if self.used == self.slots.len() {
            self.slots.push(Header::new("", []));
        }
        let h = &mut self.slots[self.used];
        h.name.clear();
        h.name.push_str(name);
        h.value.clear();
        h.value.extend_from_slice(value);
        self.used += 1;
        self.slots[..self.used].rotate_right(1);
        self.bytes += size;
    }
    fn get(&self, index: usize) -> Result<(&str, &[u8])> {
        if index == 0 {
            return Err(bad());
        }
        if index <= 61 {
            let (n, v) = STATIC[index - 1];
            Ok((n, v.as_bytes()))
        } else {
            self.slots
                .get(index - 62)
                .filter(|_| index - 62 < self.used)
                .map(|h| (h.name.as_str(), h.value.as_slice()))
                .ok_or_else(bad)
        }
    }
    fn find(&self, name: &str, value: Option<&[u8]>) -> Option<usize> {
        (1..=61 + self.used).find(|i| {
            let (n, v) = self.get(*i).unwrap();
            n == name && value.is_none_or(|x| x == v)
        })
    }
}
pub struct Decoder {
    table: Table,
    allowed: usize,
    pub max_list_size: usize,
    failed: bool,
}
impl Decoder {
    pub fn new(table_size: usize, max_list_size: usize) -> Self {
        Self {
            table: Table::new(table_size),
            allowed: table_size,
            max_list_size,
            failed: false,
        }
    }
    pub fn decode(&mut self, input: &[u8], out: &mut Vec<Header>) -> Result<()> {
        if self.failed {
            return Err(bad());
        }
        let result = self.decode_inner(input, out);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn decode_inner(&mut self, input: &[u8], out: &mut Vec<Header>) -> Result<()> {
        out.clear();
        let mut pos = 0;
        let mut size = 0;
        let mut fields = false;
        while pos < input.len() {
            let b = input[pos];
            if b & 0xe0 == 0x20 {
                if fields {
                    return Err(bad());
                }
                let n = integer(input, &mut pos, 5)?;
                if n > self.allowed {
                    return Err(bad());
                }
                self.table.resize(n);
                continue;
            }
            fields = true;
            let header = if b & 128 != 0 {
                let index = integer(input, &mut pos, 7)?;
                let (n, v) = self.table.get(index)?;
                Header {
                    name: n.to_string(),
                    value: v.to_vec(),
                }
            } else {
                let indexed = b & 64 != 0;
                let index = integer(input, &mut pos, if indexed { 6 } else { 4 })?;
                let name = if index == 0 {
                    String::from_utf8(string(
                        input,
                        &mut pos,
                        self.max_list_size.saturating_sub(size),
                    )?)
                    .map_err(|_| bad())?
                } else {
                    self.table.get(index)?.0.to_string()
                };
                let value = string(
                    input,
                    &mut pos,
                    self.max_list_size.saturating_sub(size + name.len()),
                )?;
                if indexed {
                    self.table.add(&name, &value);
                }
                Header { name, value }
            };
            size = size
                .checked_add(header.name.len() + header.value.len() + 32)
                .ok_or_else(bad)?;
            if size > self.max_list_size {
                return Err(Error::new("ENHANCE_YOUR_CALM", "header list limit"));
            }
            out.push(header);
        }
        Ok(())
    }
}
pub struct Encoder {
    table: Table,
    pending: Option<(usize, usize)>,
}
impl Encoder {
    pub fn new(table_size: usize) -> Self {
        Self {
            table: Table::new(table_size),
            pending: None,
        }
    }
    pub fn set_table_size(&mut self, size: usize) {
        self.table.resize(size);
        self.pending = Some((self.pending.map_or(size, |(min, _)| min.min(size)), size));
    }
    pub fn encode(&mut self, headers: &[Header], out: &mut Vec<u8>) {
        if let Some((min, last)) = self.pending.take() {
            encode_integer(min, 5, 0x20, out);
            if last != min {
                encode_integer(last, 5, 0x20, out);
            }
        }
        for h in headers {
            let sensitive = matches!(
                h.name.as_str(),
                "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
            );
            if !sensitive && let Some(index) = self.table.find(&h.name, Some(&h.value)) {
                encode_integer(index, 7, 128, out);
                continue;
            }
            let index = self.table.find(&h.name, None).unwrap_or(0);
            encode_integer(
                index,
                if sensitive { 4 } else { 6 },
                if sensitive { 16 } else { 64 },
                out,
            );
            if index == 0 {
                emit_string(h.name.as_bytes(), out);
            }
            emit_string(&h.value, out);
            if !sensitive {
                self.table.add(&h.name, &h.value);
            }
        }
    }
}
