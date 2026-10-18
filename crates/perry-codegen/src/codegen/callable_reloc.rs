//! Resolve worker-local operands once, while merging groups in serial order.
//! Quoted LLVM literals are opaque: user bytes can spell any compiler symbol.
use std::collections::HashMap;

pub(crate) struct Relocations {
    pub(crate) symbols: HashMap<String, String>,
    ic_base: u32,
    alias_base: u32,
    string_base: u32,
    site_hash: u64,
    prefix: String,
}
impl Relocations {
    pub(crate) fn new(prefix: &str, ic_base: u32, alias_base: u32, string_base: u32) -> Self {
        let mut hash = 0x811c9dc5u32;
        for b in prefix.bytes() {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        Self {
            symbols: HashMap::new(),
            ic_base,
            alias_base,
            string_base,
            site_hash: u64::from(hash & 0x7fff_ffff) << 32,
            prefix: prefix.into(),
        }
    }
    pub(crate) fn add_definition(&self, line: &str, map: &mut HashMap<String, String>) {
        let Some(name) = line
            .strip_prefix('@')
            .and_then(|s| s.split_whitespace().next())
        else {
            return;
        };
        let raw_string = if self.prefix.is_empty() {
            ".str.".to_string()
        } else {
            format!("{}_.str.", self.prefix)
        };
        if let Some(num) = name
            .strip_prefix(&raw_string)
            .and_then(|s| s.parse::<u32>().ok())
        {
            map.insert(
                name.into(),
                format!("{raw_string}{}", self.string_base + num),
            );
            return;
        }
        // Every item in these families is minted by FnCtx::ic_site_counter.
        // The final digit run is the site, optionally followed by a role.
        const FAMILIES: &[&str] = &[
            "perry_ic_",
            "perry_concat_site_",
            "perry_regexp_site_",
            "perry_private_site_",
            "perry_construct_site_",
            "perry_typed_feedback_",
            "perry_literal_",
            "perry_enumimg_",
            "perry_births_",
            "perry_accessor_candidate_",
            "perry_const_arr_",
            "perry_node_submod_str_",
            "perry_typed_parse_keys_",
            "perry_typed_shape_mask_",
            "perry_v8_",
        ];
        if !FAMILIES.iter().any(|p| name.starts_with(p)) {
            return;
        }
        let bytes = name.as_bytes();
        let Some(end) = bytes.iter().rposition(u8::is_ascii_digit).map(|p| p + 1) else {
            return;
        };
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let num: u32 = name[start..end].parse().expect("site number");
        map.insert(
            name.into(),
            format!("{}{}{}", &name[..start], self.ic_base + num, &name[end..]),
        );
    }
    pub(crate) fn symbol(&self, token: &mut String) {
        if let Some(new) = self.symbols.get(token.as_str()) {
            *token = new.clone();
        }
    }
    pub(crate) fn text(&self, text: &mut String) {
        let bytes = text.as_bytes();
        let mut out = String::new();
        let mut cursor = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            if bytes[i] == b'@' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || b"_.$-".contains(&bytes[end]))
                {
                    end += 1;
                }
                if let Some(new) = self.symbols.get(&text[start..end]) {
                    out.push_str(&text[cursor..start]);
                    out.push_str(new);
                    cursor = end;
                }
                i = end.max(i + 1);
                continue;
            }
            const SITE: &str = "__perry_callable_site_";
            if text[i..].starts_with(SITE) {
                let start = i + SITE.len();
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let local: u32 = text[start..end].parse().expect("callable site operand");
                out.push_str(&text[cursor..i]);
                out.push_str(&(self.site_hash | u64::from(self.ic_base + local)).to_string());
                cursor = end;
                i = end;
                continue;
            }
            // Metadata numbers are relative only after these named operands.
            let metadata = ["!alias.scope !", "!noalias !"]
                .into_iter()
                .find(|prefix| text[i..].starts_with(prefix));
            if let Some(prefix) = metadata {
                let start = i + prefix.len();
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_digit() {
                    end += 1;
                }
                let local: u32 = text[start..end].parse().expect("alias metadata operand");
                out.push_str(&text[cursor..start]);
                out.push_str(&(local + self.alias_base).to_string());
                cursor = end;
                i = end;
                continue;
            }
            // Only ASCII can begin a compiler token; keep UTF-8 boundaries.
            i += text[i..].chars().next().unwrap().len_utf8();
        }
        if cursor != 0 {
            out.push_str(&text[cursor..]);
            *text = out;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relocation_keeps_quoted_bytes_and_integer_literals() {
        let mut r = Relocations::new("module", 100, 7, 5);
        r.symbols.insert(
            "module_.str.1.handle".into(),
            "module_.str.10.handle".into(),
        );
        let mut text = "@module_.str.1.handle @module_.str.11.handle c\"@module_.str.1.handle __perry_callable_site_3\" i64 123, !alias.scope !201, !noalias !301 ; λ".to_string();
        r.text(&mut text);
        assert_eq!(text, "@module_.str.10.handle @module_.str.11.handle c\"@module_.str.1.handle __perry_callable_site_3\" i64 123, !alias.scope !208, !noalias !308 ; λ");
        let mut site = "__perry_callable_site_3".into();
        r.text(&mut site);
        assert_eq!(site, (r.site_hash | 103).to_string());
    }
}

#[cfg(test)]
mod empty_prefix_tests {
    use super::*;
    #[test]
    fn callable_raw_constants_with_empty_prefix_are_relocated() {
        let r = Relocations::new("", 0, 0, 8);
        let mut symbols = HashMap::new();
        r.add_definition(
            "@.str.3 = private unnamed_addr constant [2 x i8] c\"x\\00\"",
            &mut symbols,
        );
        assert_eq!(symbols.get(".str.3").map(String::as_str), Some(".str.11"));
    }
}
