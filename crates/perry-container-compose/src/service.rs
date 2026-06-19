use md5::{Digest, Md5};

pub fn generate_name(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let short_hash = &hash[..8];
    let random_suffix: u32 = rand::random();
    format!("{}-{:08x}", short_hash, random_suffix)
}

pub fn service_container_name(
    service: &crate::types::ComposeService,
    _service_name: &str,
) -> String {
    if let Some(name) = service.container_name.as_ref() {
        return name.clone();
    }

    let image = service.image.as_deref().unwrap_or("unknown");
    let mut hasher = Md5::new();
    hasher.update(image.as_bytes());
    let hash = hex::encode(hasher.finalize());
    let short_hash = &hash[..8];

    let random_suffix: u32 = rand::random();

    format!("{}-{:08x}", short_hash, random_suffix)
}

pub struct ServiceState {
    pub id: String,
    pub name: String,
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ComposeService;

    #[test]
    fn test_service_container_name_format() {
        let svc = ComposeService {
            image: Some("redis:7".to_string()),
            ..Default::default()
        };
        let name = service_container_name(&svc, "cache");

        // Format: {md5_8chars}-{random_hex8}
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 8);
    }

    #[test]
    fn test_service_container_name_stability() {
        let svc = ComposeService {
            image: Some("postgres:16".to_string()),
            ..Default::default()
        };

        let n1 = service_container_name(&svc, "db");
        let n2 = service_container_name(&svc, "db");

        let parts1: Vec<&str> = n1.split('-').collect();
        let parts2: Vec<&str> = n2.split('-').collect();

        // Image hash (part 0) should be stable for the same image
        assert_eq!(parts1[0], parts2[0]);
        // Random suffix (part 1) should vary
        assert_ne!(parts1[1], parts2[1]);
    }

    #[test]
    fn test_service_container_name_override() {
        let svc = ComposeService {
            container_name: Some("my-custom-name".to_string()),
            ..Default::default()
        };
        let name = service_container_name(&svc, "ignored");
        assert_eq!(name, "my-custom-name");
    }
}
