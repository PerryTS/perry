//! String-pool metadata constants, emitted before the init function borrows
//! the module. Kept separate for the codegen file-size limit.
use super::{image, IMAGE_STRING_HEADER_BYTES};
use crate::module::LlModule;
use crate::strings::StringPool;

pub(super) fn packed_keys<'a>(
    llmod: &mut LlModule,
    module_prefix: &str,
    packed_keys: impl ExactSizeIterator<Item = &'a str>,
) -> Vec<String> {
    // Per-class packed-keys constants (rodata) — referenced by the
    // js_build_class_keys_array call below at module init.
    // Naming: `@perry_class_keys_packed_<modprefix>__<idx>` so we
    // don't collide with anything else.
    let mut packed_global_names: Vec<String> = Vec::with_capacity(packed_keys.len());
    for (idx, packed) in packed_keys.enumerate() {
        if packed.is_empty() {
            packed_global_names.push(String::new());
            continue;
        }
        let bytes = packed.as_bytes();
        let mut lit = String::with_capacity(bytes.len() + 8);
        lit.push_str("c\"");
        for &b in bytes {
            if (32..127).contains(&b) && b != b'"' && b != b'\\' {
                lit.push(b as char);
            } else {
                lit.push('\\');
                lit.push_str(&format!("{:02X}", b));
            }
        }
        lit.push_str("\\00\"");
        let name = format!("perry_class_keys_packed_{}__{}", module_prefix, idx);
        llmod.add_named_string_constant(&name, bytes.len() + 1, &lit);
        packed_global_names.push(name);
    }

    packed_global_names
}

pub(super) fn function_names(
    llmod: &mut LlModule,
    strings: &StringPool,
    module_prefix: &str,
    image_literals: bool,
    user_fn_display_names: &[(String, String)],
) -> Vec<(String, String, usize)> {
    // Pre-allocate string constants for function-name registration. Same
    // borrow-ordering constraint as the class-name constants below: we
    // must mint the rodata globals BEFORE `init_fn` claims `&mut llmod`.
    // Each entry becomes one relative record in the module's batched function
    // name descriptor table. See #1202 and #11927.
    let mut user_fn_name_constants: Vec<(String, String, usize)> = Vec::new();
    // Deduplicated by CONTENT (#9486): the same display name is now registered
    // against several symbols — a top-level function's wrapper and its body,
    // a class method and its `__perry_wrap_*` twin — and `add_string_constant`
    // mints a fresh `@.str.N` per call, so without this every extra
    // registration also cost a duplicate copy of the name in rodata.
    // Deterministic: the map only reuses a global the loop already minted in
    // its (already sorted) input order, so emission order is unchanged (#7622).
    //
    // In an executable the name bytes are an image string's text, named by an
    // internal alias: the image string is the function's own `name` value
    // (`FN_NAME_IMAGE`), read with no allocation.
    let mut name_constant_cache: std::collections::HashMap<&str, (String, usize)> =
        std::collections::HashMap::new();
    for (wrapper_sym, display_name) in user_fn_display_names {
        if wrapper_sym.is_empty() || display_name.is_empty() {
            continue;
        }
        let (const_name, byte_len) = match name_constant_cache.get(display_name.as_str()) {
            Some(hit) => hit.clone(),
            None => {
                let minted = if image_literals {
                    // A pool entry's image is emitted above; any other name
                    // emits its own (weak_odr: one per spelling per program).
                    let image = match strings.lookup(display_name) {
                        Some(entry) => image::pointer(entry),
                        None => image::emit(
                            llmod,
                            &crate::strings::StringPool::detached_entry(display_name),
                        ),
                    };
                    if !llmod.target_triple.contains("apple")
                        && !llmod.target_triple.contains("wasm")
                    {
                        // A local alias to a COMDAT image becomes a section
                        // relocation on ELF. A losing group would leave name
                        // metadata pointing into discarded data. Reference the
                        // weak image itself so the linker resolves its winner.
                        (
                            format!(
                                "getelementptr (i8, ptr {image}, i64 {IMAGE_STRING_HEADER_BYTES})"
                            ),
                            display_name.len(),
                        )
                    } else {
                        let alias = if module_prefix.is_empty() {
                            format!(".fnname.{}", name_constant_cache.len())
                        } else {
                            format!("{module_prefix}_.fnname.{}", name_constant_cache.len())
                        };
                        llmod.add_raw_global(format!(
                            "@{alias} = internal alias i8, getelementptr (i8, ptr {image}, i64 {IMAGE_STRING_HEADER_BYTES})"
                        ));
                        (alias, display_name.len())
                    }
                } else {
                    llmod.add_string_constant(display_name)
                };
                name_constant_cache.insert(display_name.as_str(), minted.clone());
                minted
            }
        };
        llmod.note_fn_info(wrapper_sym, |f| {
            f.name = Some(crate::fn_info::RetainedName {
                global: const_name.clone(),
                byte_len,
                image: image_literals,
            });
        });
        user_fn_name_constants.push((wrapper_sym.clone(), const_name, byte_len));
    }

    user_fn_name_constants
}
