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
