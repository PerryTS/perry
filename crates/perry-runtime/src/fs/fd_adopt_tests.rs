//! Inherited (adopted) descriptors are not owned by the registry: only the
//! program's explicit close, or a socket built on the fd, closes them.
use super::*;
use std::os::fd::FromRawFd;

fn pipe() -> (i32, i32) {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    (fds[0], fds[1])
}

fn is_open(fd: i32) -> bool {
    unsafe { libc::fcntl(fd, libc::F_GETFD) >= 0 }
}

fn adopted(fd: i32) -> FdEntry {
    // SAFETY: the test owns `fd`; the entry is adopted, so it never closes it.
    let file = unsafe { fs::File::from_raw_fd(fd) };
    FdEntry::Adopted(std::mem::ManuallyDrop::new(file))
}

#[test]
fn adopted_fds_survive_registry_and_thread_teardown() {
    let (r, w) = pipe();
    std::thread::spawn(move || {
        let mut table = FdTable::default();
        adopt_inherited_fds(&mut table);
        assert!(table.contains_key(&r) && table.contains_key(&w));
        // The thread's registry and `table` are dropped as it exits.
    })
    .join()
    .unwrap();
    let (r_open, w_open) = (is_open(r), is_open(w));
    // SAFETY: close the test's own descriptors (only if still open).
    unsafe {
        if r_open {
            libc::close(r);
        }
        if w_open {
            libc::close(w);
        }
    }
    assert!(r_open && w_open, "teardown closed an adopted descriptor");
}

#[test]
fn registry_built_on_a_thread_leaves_inherited_fds_open() {
    // The real path: the first registry of the process claims the
    // inherited descriptors (the claim may already be taken by an earlier
    // test, in which case the descriptors are simply untouched).
    let (r, w) = pipe();
    std::thread::spawn(|| {
        assert!(fd_is_registered(1));
    })
    .join()
    .unwrap();
    let (r_open, w_open) = (is_open(r), is_open(w));
    unsafe {
        libc::close(r);
        libc::close(w);
    }
    assert!(r_open && w_open);
}

#[test]
fn explicit_close_of_an_adopted_fd_closes_it_and_drops_the_entry() {
    let _global = crate::gc::global_side_table_test_lock();
    let (r, w) = pipe();
    FD_REGISTRY.with(|t| t.borrow_mut().insert(r, adopted(r)));
    assert!(fd_is_registered(r));
    assert!(is_open(r));
    assert_eq!(js_fs_close_sync(r as f64), 1);
    assert!(!fd_is_registered(r));
    assert!(!is_open(r), "closeSync left the adopted descriptor open");
    unsafe { libc::close(w) };
}

#[test]
fn taking_an_adopted_fd_hands_over_an_owner_that_closes_it() {
    let _global = crate::gc::global_side_table_test_lock();
    let (r, w) = pipe();
    FD_REGISTRY.with(|t| t.borrow_mut().insert(r, adopted(r)));
    let file = take_registered_fd(r).unwrap();
    assert!(!fd_is_registered(r));
    assert!(is_open(r));
    drop(file);
    assert!(!is_open(r));
    unsafe { libc::close(w) };
}

#[test]
fn a_registry_dropped_entry_closes_owned_but_not_adopted() {
    let (ar, aw) = pipe();
    let (or, ow) = pipe();
    let mut table = FdTable::default();
    table.insert(ar, adopted(ar));
    table.insert(or, unsafe { fs::File::from_raw_fd(or) });
    drop(table);
    assert!(is_open(ar));
    assert!(!is_open(or));
    unsafe {
        libc::close(ar);
        libc::close(aw);
        libc::close(ow);
    }
}
