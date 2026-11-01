use super::std_fd_registry;
use std::os::fd::AsRawFd;

#[test]
fn registry_teardown_closes_only_its_owned_standard_descriptor_duplicates() {
    let registry = std_fd_registry();
    let owned: Vec<_> = registry
        .iter()
        .map(|(fd, file)| (*fd, file.as_raw_fd()))
        .collect();
    assert_eq!(
        owned.len(),
        3,
        "test process must have all standard streams"
    );
    for &(standard, duplicate) in &owned {
        assert_ne!(standard, duplicate);
        assert!(unsafe { libc::fcntl(duplicate, libc::F_GETFD) } >= 0);
    }
    drop(registry);
    for (standard, duplicate) in owned {
        assert!(unsafe { libc::fcntl(standard, libc::F_GETFD) } >= 0);
        assert_eq!(unsafe { libc::fcntl(duplicate, libc::F_GETFD) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EBADF)
        );
    }
}
