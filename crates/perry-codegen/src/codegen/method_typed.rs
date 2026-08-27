//! Typed (f64 / f64-receiver / i1 / i32 / string) internal method clones.
//!
//! Child module of `method.rs`, split out to stay under the 2,000-line file
//! gate; `use super::*` keeps the parent's private helpers reachable.

use super::*;

/// Compile the internal typed-f64 clone for a conservatively eligible instance
/// method. The public/generic method body keeps the usual
/// `double(this, args...) -> double` ABI and remains the only symbol registered
/// in runtime vtables.
pub(in crate::codegen) fn compile_typed_f64_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_f64_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-f64 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_f64_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

/// Compile the internal typed-f64 receiver clone for an exact own instance
/// method. The clone takes a raw receiver handle (`i64`) plus raw numeric
/// method arguments; callers must compose the method-direct guard with raw-f64
/// class-field guards for every receiver field before entering it.
pub(in crate::codegen) fn compile_typed_f64_receiver_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
    receiver: &TypedReceiverMethodInfo,
    header_skip: u64,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_f64_receiver_method_name(&generic_name);
    let mut params: Vec<(LlvmType, String)> = Vec::with_capacity(method.params.len() + 1);
    params.push((I64, "%this_obj".to_string()));
    for p in &method.params {
        params.push((DOUBLE, format!("%arg{}", p.id)));
    }
    let lf = llmod.define_function(&llvm_name, DOUBLE, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_f64_receiver_body(blk, &method.params, &method.body, receiver, header_skip)?
    };
    lf.block_mut(0).unwrap().ret(DOUBLE, &value);
    Ok(())
}

/// Compile the internal typed-i1 clone for a conservatively eligible instance
/// method. Runtime vtables still register only the generic method symbol; this
/// clone is only called from guarded exact own-method sites.
pub(in crate::codegen) fn compile_typed_i1_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_i1_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-i1 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I1, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i1_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I1, &value);
    Ok(())
}

/// Compile the internal typed-i32 clone for a conservatively eligible instance
/// method. The public method symbol remains a JSValue trampoline registered in
/// the vtable; this clone is reached only after exact method and Int32 guards.
pub(in crate::codegen) fn compile_typed_i32_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_i32_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-i32 method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I32, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_i32_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I32, &value);
    Ok(())
}

/// Compile the internal typed-string clone for a conservatively eligible
/// instance method. The clone passes raw `StringHeader*` handles as i64; the
/// public method symbol remains a JSValue trampoline registered in vtables.
pub(in crate::codegen) fn compile_typed_string_method(
    llmod: &mut LlModule,
    class: &perry_hir::Class,
    method: &Function,
    methods: &HashMap<(String, String), String>,
) -> Result<()> {
    let generic_name = methods
        .get(&(class.name.clone(), method.name.clone()))
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "method '{}::{}' missing from registry",
                class.name,
                method.name
            )
        })?;
    let llvm_name = typed_string_method_name(&generic_name);
    let param_reps = typed_param_reps_for_params(&method.params).ok_or_else(|| {
        anyhow!(
            "typed-string method '{}::{}' has unsupported parameter",
            class.name,
            method.name
        )
    })?;
    let params: Vec<(LlvmType, String)> = method
        .params
        .iter()
        .zip(param_reps.iter())
        .map(|(p, rep)| (rep.llvm_ty(), format!("%arg{}", p.id)))
        .collect();
    let lf = llmod.define_function(&llvm_name, I64, params);
    lf.linkage = "internal".to_string();
    lf.force_inline = true;
    let _ = lf.create_block("entry");

    let value = {
        let blk = lf.block_mut(0).unwrap();
        lower_typed_string_body(blk, &method.params, &method.body)?
    };
    lf.block_mut(0).unwrap().ret(I64, &value);
    Ok(())
}
