use super::*;
use wasmtime::{Engine, Instance, Module, Store, StoreLimitsBuilder};

#[test]
fn canonical_heap_reuses_buffers_preserves_realloc_and_rejects_double_free() {
    let engine = Engine::default();
    let module = Module::from_binary(&engine, &wat::parse_str(HEAP).unwrap()).unwrap();
    let mut store = Store::new(
        &engine,
        StoreLimitsBuilder::new()
            .memory_size(16 * 1024 * 1024)
            .trap_on_grow_failure(true)
            .build(),
    );
    store.limiter(|limits| limits);
    let instance = Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "alloc")
        .unwrap();
    let free = instance
        .get_typed_func::<u32, ()>(&mut store, "free")
        .unwrap();
    let realloc = instance
        .get_typed_func::<(u32, u32, u32, u32), u32>(&mut store, "realloc")
        .unwrap();
    let original = alloc.call(&mut store, 31).unwrap();
    assert_eq!(original % 16, 0);
    let expected = (0..31).map(|i| (i * 31 + 7) as u8).collect::<Vec<_>>();
    memory
        .write(&mut store, original as usize, &expected)
        .unwrap();
    let grown = realloc
        .call(&mut store, (original, 31, 8, 3 * 1024 * 1024 + 137))
        .unwrap();
    assert_ne!(grown, original);
    let mut bytes = vec![0; 31];
    memory.read(&store, grown as usize, &mut bytes).unwrap();
    assert_eq!(bytes, expected);
    free.call(&mut store, grown).unwrap();
    assert_eq!(alloc.call(&mut store, 31).unwrap(), original);
    free.call(&mut store, original).unwrap();
    let sizes = [1, 7, 17, 33, 96, 4096, 65537];
    for size in sizes {
        let p = alloc.call(&mut store, size).unwrap();
        free.call(&mut store, p).unwrap();
    }
    let pages = memory.size(&store);
    for _ in 0..1000 {
        for size in sizes {
            let p = alloc.call(&mut store, size).unwrap();
            assert_eq!(p % 16, 0);
            free.call(&mut store, p).unwrap();
        }
    }
    assert_eq!(
        memory.size(&store),
        pages,
        "freed canonical buffers must not cause unbounded growth"
    );
    assert!(free.call(&mut store, original).is_err());
    assert!(realloc.call(&mut store, (0, 0, 3, 1)).is_err());
}

#[test]
fn failed_helper_growth_keeps_existing_values_and_allocator_usable() {
    let engine = Engine::default();
    let module = Module::from_binary(&engine, &wat::parse_str(HEAP).unwrap()).unwrap();
    let mut store = Store::new(
        &engine,
        StoreLimitsBuilder::new()
            .memory_size(65536)
            .trap_on_grow_failure(true)
            .build(),
    );
    store.limiter(|limits| limits);
    let instance = Instance::new(&mut store, &module, &[]).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let alloc = instance
        .get_typed_func::<u32, u32>(&mut store, "alloc")
        .unwrap();
    let free = instance
        .get_typed_func::<u32, ()>(&mut store, "free")
        .unwrap();
    let p = alloc.call(&mut store, 16).unwrap();
    memory.write(&mut store, p as usize, b"retained").unwrap();
    assert!(alloc.call(&mut store, 1024 * 1024).is_err());
    let mut bytes = [0; 8];
    memory.read(&store, p as usize, &mut bytes).unwrap();
    assert_eq!(&bytes, b"retained");
    let q = alloc.call(&mut store, 16).unwrap();
    assert_ne!(p, q);
    assert_eq!(memory.size(&store), 1);
    free.call(&mut store, p).unwrap();
    free.call(&mut store, q).unwrap();
}
