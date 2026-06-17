use perry_container_compose::yaml::*;
use std::collections::HashMap;

// Feature: perry-container | Layer: unit | Req: 7.8 | Property: 6
#[test]
fn test_interpolate_basic() {
    let mut env = HashMap::new();
    env.insert("VAR".into(), "value".into());
    let input = "hello ${VAR}";
    let output = interpolate(input, &env);
    assert_eq!(output, "hello value");
}

// Feature: perry-container | Layer: unit | Req: 7.8 | Property: 6
#[test]
fn test_interpolate_default() {
    let env = HashMap::new();
    let input = "hello ${VAR:-world}";
    let output = interpolate(input, &env);
    assert_eq!(output, "hello world");
}

// Feature: perry-container | Layer: unit | Req: 7.9 | Property: -
#[test]
fn test_parse_dotenv() {
    let content = "KEY=VAL\n#comment\nEMPTY=\n";
    let env = parse_dotenv(content);
    assert_eq!(env.get("KEY").unwrap(), "VAL");
    assert_eq!(env.get("EMPTY").unwrap(), "");
    assert!(!env.contains_key("comment"));
}

// Coverage Table:
// | Requirement | Test name | Layer |
// |-------------|-----------|-------|
// | 7.8         | test_interpolate_basic | unit |
// | 7.8         | test_interpolate_default | unit |
// | 7.9         | test_parse_dotenv | unit |
