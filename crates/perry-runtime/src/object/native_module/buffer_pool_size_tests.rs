// #11471: `Buffer.poolSize` is per realm, so a worker's write (possibly of
// a heap value) never reaches another thread.
#[test]
fn buffer_pool_size_writes_stay_on_their_thread() {
    let written = std::thread::spawn(|| {
        super::set_buffer_pool_size(1.0);
        super::buffer_pool_size()
    })
    .join()
    .unwrap();
    assert_eq!(written, 1.0);
    let elsewhere = std::thread::spawn(super::buffer_pool_size).join().unwrap();
    assert_eq!(elsewhere, 65536.0);
}

#[test]
fn materialized_buffer_pool_size_uses_the_constructor_property() {
    std::thread::spawn(|| {
        let _no_move = crate::gc::GcSuppressScope::new();
        let constructor = super::buffer_constructor_value();
        let ptr = (constructor.to_bits() & crate::value::POINTER_MASK) as usize;
        crate::closure::closure_set_dynamic_prop(ptr, "poolSize", 8192.0);
        assert_eq!(super::buffer_pool_size(), 8192.0);
        super::set_buffer_pool_size(16384.0);
        assert_eq!(
            crate::closure::closure_get_dynamic_prop(ptr, "poolSize"),
            16384.0
        );
        assert_eq!(super::buffer_pool_size(), 16384.0);
    })
    .join()
    .unwrap();
}
