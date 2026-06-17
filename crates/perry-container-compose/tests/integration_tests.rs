//! Integration tests for perry-container-compose.
//!
//! These tests require a running container backend and are gated
//! by `#[cfg(feature = "integration-tests")]`.
//!
//! The unit tests and property tests are in the modules themselves
//! and in `tests/round_trip.rs`.

#[cfg(feature = "integration-tests")]
mod integration {
    use perry_container_compose::compose::resolve_startup_order;
    use perry_container_compose::types::{ComposeService, ComposeSpec, DependsOnSpec};
    use perry_container_compose::yaml::{interpolate, parse_compose_yaml, parse_dotenv};
    use std::collections::HashMap;

    #[test]
    fn test_parse_simple_compose() {
        let yaml = r#"
services:
  web:
    image: nginx:alpine
    ports:
      - "8080:80"
"#;
        let spec = ComposeSpec::parse_str(yaml).expect("parse failed");
        assert!(spec.services.contains_key("web"));
        assert_eq!(spec.services["web"].image.as_deref(), Some("nginx:alpine"));
    }

    #[test]
    fn test_parse_multi_service_with_deps() {
        let yaml = r#"
services:
  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: secret
  web:
    image: myapp:latest
    depends_on:
      - db
    ports:
      - "3000:3000"
"#;
        let spec = ComposeSpec::parse_str(yaml).expect("parse failed");
        assert_eq!(spec.services.len(), 2);
        let web = &spec.services["web"];
        let deps = web.depends_on.as_ref().unwrap().service_names();
        assert!(deps.contains(&"db".to_string()));
    }

    #[test]
    fn test_topological_order_linear() {
        let yaml = r#"
services:
  c:
    image: c
    depends_on: [b]
  b:
    image: b
    depends_on: [a]
  a:
    image: a
"#;
        let spec = ComposeSpec::parse_str(yaml).unwrap();
        let order = resolve_startup_order(&spec).unwrap();
        let pos = |s: &str| order.iter().position(|n| n == s).unwrap();
        assert!(pos("a") < pos("b"), "a before b");
        assert!(pos("b") < pos("c"), "b before c");
    }

    #[test]
    fn test_circular_dependency_detected() {
        let yaml = r#"
services:
  a:
    image: a
    depends_on: [b]
  b:
    image: b
    depends_on: [a]
"#;
        let spec = ComposeSpec::parse_str(yaml).unwrap();
        let result = resolve_startup_order(&spec);
        assert!(result.is_err());
    }

    #[test]
    fn test_env_interpolation() {
        let mut env = HashMap::new();
        env.insert("DB_USER".to_string(), "admin".to_string());
        env.insert("DB_PASS".to_string(), "s3cr3t".to_string());

        let yaml = "  url: postgres://${DB_USER}:${DB_PASS}@localhost/db";
        let result = interpolate(yaml, &env);
        assert_eq!(result, "  url: postgres://admin:s3cr3t@localhost/db");
    }

    #[test]
    fn test_dotenv_parse() {
        let content = "HOST=localhost\nPORT=5432\n# ignored\n\nEMPTY=";
        let env = parse_dotenv(content);
        assert_eq!(env["HOST"], "localhost");
        assert_eq!(env["PORT"], "5432");
        assert_eq!(env["EMPTY"], "");
    }

    #[test]
    fn test_compose_merge_override() {
        let base_yaml = r#"
services:
  web:
    image: nginx:1.0
  db:
    image: postgres:15
"#;
        let override_yaml = r#"
services:
  web:
    image: nginx:2.0
"#;
        let mut base = ComposeSpec::parse_str(base_yaml).unwrap();
        let overlay = ComposeSpec::parse_str(override_yaml).unwrap();
        base.merge(overlay);

        assert_eq!(base.services["web"].image.as_deref(), Some("nginx:2.0"));
        assert!(base.services.contains_key("db"));
    }
}
