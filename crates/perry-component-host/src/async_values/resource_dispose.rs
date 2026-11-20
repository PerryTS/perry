//! A single private helper drops resources using their original nominal types.
//! No memory, invented public future types or raw resource representations are
//! exposed to the embedding. Async entry permits owner-stack host dispatch.
use super::reader_types::{Resources, Types};
use anyhow::Result;
use perry_wit::World;

pub(super) fn generate(world: &World, resources: &Resources) -> Result<String> {
    let mut types = Types::new(world, resources);
    let mut intrinsics = String::new();
    let mut imports = String::new();
    let mut functions = String::new();
    let mut exports = String::new();
    let mut interface = String::new();
    for (index, key) in resources.keys().enumerate() {
        let ty = types.resource(key, 0)?;
        intrinsics += &format!("(core func $drop{index} (canon resource.drop {ty}))\n");
        imports += &format!("(import \"host\" \"drop-{index}\" (func $drop{index} (param i32)))\n");
        functions += &format!("(func (export \"dispose-{index}\") (param i32) (result i32) (call $drop{index} (local.get 0)) (call $return) (i32.const 0))\n");
        exports += &format!("(export \"drop-{index}\" (func $drop{index}))\n");
        interface += &format!("(func (export \"dispose-{index}\") async (param \"value\" (own {ty})) (canon lift (core func $code \"dispose-{index}\") async (callback (core func $code \"callback\"))))\n");
    }
    Ok(format!(
        r#"(component
        {}
        {intrinsics}
        (core func $return (canon task.return))
        (core module $code
            {imports}
            (import "host" "return" (func $return))
            {functions}
            (func (export "callback") (param i32 i32 i32) (result i32) unreachable))
        (core instance $code (instantiate $code (with "host" (instance
            {exports} (export "return" (func $return))))))
        {interface})"#,
        types.imports()
    ))
}
