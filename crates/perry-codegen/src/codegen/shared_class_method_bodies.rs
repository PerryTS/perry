//! Fresh method entries and direct calls share the explicit-owner body.
use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64};

pub(super) fn share(module: &mut LlModule, public: &str, class_id: u32) {
    let owner_name = super::helpers::generator_method_owner_name(public);
    let generic = super::typed_abi::generic_method_body_name(public);
    let body_name = if module.function_named(&generic).is_some() {
        generic
    } else {
        public.into()
    };
    let Some(old) = module.function_named(&body_name) else {
        return;
    };
    let params = old.params.clone();
    let linkage = old.linkage.clone();
    let mut wrapper = crate::function::LlFunction::new(body_name, DOUBLE, params.clone());
    wrapper.linkage = linkage;
    wrapper.enable_shadow_frame(0);
    let b = wrapper.create_block("entry");
    let owner = b.call(DOUBLE, "js_class_value", &[(I32, &class_id.to_string())]);
    let owner = crate::expr::unbox_to_i64(b, &owner);
    let mut args = vec![(I64, owner.as_str())];
    args.extend(params.iter().map(|(ty, name)| (*ty, name.as_str())));
    let value = b.call(DOUBLE, &owner_name, &args);
    b.ret(DOUBLE, &value);
    module.replace_defined_function(wrapper);
    // Keep one implementation even for a small source body. Both its entry
    // and direct-call adapter retain their own ABI and lexical owner.
    for i in 0..module.function_count() {
        let f = module.function_mut(i).unwrap();
        if f.name == owner_name {
            f.force_inline = false;
            f.pre_statepoint_inline = false;
            f.inline_hint = false;
            f.no_inline = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fresh_direct_calls_and_callable_entries_keep_one_implementation() {
        let mut m = LlModule::new("x86_64-unknown-linux-gnu");
        m.declare_function("js_class_value", DOUBLE, &[I32]);
        m.declare_function("discarded_body_work", DOUBLE, &[]);
        let f = m.define_function("method", DOUBLE, vec![(DOUBLE, "%this_arg".into())]);
        let b = f.create_block("entry");
        let v = b.call(DOUBLE, "discarded_body_work", &[]);
        b.ret(DOUBLE, &v);
        let f = m.define_function(
            "method$generator_owner",
            DOUBLE,
            vec![(I64, "%owner".into()), (DOUBLE, "%this_arg".into())],
        );
        f.create_block("entry").ret(DOUBLE, "%this_arg");
        share(&mut m, "method", 123);
        let ir = m.to_ir();
        assert!(!ir.contains("call double @discarded_body_work"));
        assert!(ir.contains("call double @method$generator_owner(i64 "));
        assert!(
            m.function_named("method$generator_owner")
                .unwrap()
                .no_inline
        );
    }
}
