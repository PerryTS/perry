use perry_container_compose::service::generate_name;

#[test]
fn test_generate_name_format() {
    let name = generate_name("image: nginx");
    // Format: {md5_8chars}-{random_hex}
    let parts: Vec<&str> = name.split('-').collect();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 8);
}

#[test]
fn test_generate_name_stable_per_yaml() {
    let name1 = generate_name("image: nginx");
    let name2 = generate_name("image: nginx");
    // Prefix is md5 hash, so same input → same prefix
    assert_eq!(
        name1.split('-').next().unwrap(),
        name2.split('-').next().unwrap()
    );
}

#[test]
fn test_generate_name_different_per_yaml() {
    let name1 = generate_name("image: nginx");
    let name2 = generate_name("image: redis");
    assert_ne!(
        name1.split('-').next().unwrap(),
        name2.split('-').next().unwrap()
    );
}
