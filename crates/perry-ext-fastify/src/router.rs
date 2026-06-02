//! Route pattern parsing and matching.
//!
//! Supports:
//! - Static segments: `/users`, `/api/v1`
//! - Parameters: `/users/:id`, `/posts/:postId/comments/:commentId`
//! - Wildcards: `/static/*` (captures rest of path)

use std::collections::HashMap;

/// A segment in a route pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// Static path segment (e.g., "users").
    Static(String),
    /// Named parameter (e.g., ":id" captures "id").
    Param(String),
    /// Wildcard captures rest of path.
    Wildcard,
}

/// Parsed route pattern for efficient matching.
#[derive(Debug, Clone)]
pub struct RoutePattern {
    /// Pattern segments.
    pub segments: Vec<Segment>,
    /// Original pattern string.
    pub raw: String,
}

impl RoutePattern {
    /// Parse a route pattern string into segments.
    pub fn parse(path: &str) -> Self {
        let mut segments = Vec::new();
        let path = path.trim_start_matches('/');

        if path.is_empty() {
            return Self {
                segments,
                raw: "/".to_string(),
            };
        }

        for part in path.split('/') {
            if part.is_empty() {
                continue;
            }
            let segment = if let Some(rest) = part.strip_prefix(':') {
                Segment::Param(rest.to_string())
            } else if part == "*" {
                Segment::Wildcard
            } else {
                Segment::Static(part.to_string())
            };
            segments.push(segment);
        }

        Self {
            segments,
            raw: path.to_string(),
        }
    }

    /// Match a request path against this pattern.
    ///
    /// Returns `Some(params)` if the path matches; `None` otherwise.
    pub fn match_path(&self, path: &str) -> Option<HashMap<String, String>> {
        let path = path.trim_start_matches('/');
        let path = path.split('?').next().unwrap_or(path);
        let path_parts: Vec<&str> = if path.is_empty() {
            Vec::new()
        } else {
            path.split('/').filter(|s| !s.is_empty()).collect()
        };

        if self.segments.is_empty() {
            return if path_parts.is_empty() {
                Some(HashMap::new())
            } else {
                None
            };
        }

        let mut params = HashMap::new();
        let mut path_idx = 0;

        for segment in &self.segments {
            match segment {
                Segment::Static(expected) => {
                    if path_idx >= path_parts.len() || path_parts[path_idx] != expected {
                        return None;
                    }
                    path_idx += 1;
                }
                Segment::Param(name) => {
                    if path_idx >= path_parts.len() {
                        return None;
                    }
                    params.insert(name.clone(), path_parts[path_idx].to_string());
                    path_idx += 1;
                }
                Segment::Wildcard => {
                    let rest: String = path_parts[path_idx..].join("/");
                    params.insert("*".to_string(), rest);
                    return Some(params);
                }
            }
        }

        if path_idx == path_parts.len() {
            Some(params)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_route_matches() {
        let pattern = RoutePattern::parse("/users");
        assert!(pattern.match_path("/users").is_some());
        assert!(pattern.match_path("/posts").is_none());
        assert!(pattern.match_path("/users/123").is_none());
    }

    #[test]
    fn param_route_extracts() {
        let pattern = RoutePattern::parse("/users/:id");
        let params = pattern.match_path("/users/42").unwrap();
        assert_eq!(params.get("id"), Some(&"42".to_string()));
        assert!(pattern.match_path("/users").is_none());
    }

    #[test]
    fn wildcard_captures_rest() {
        let pattern = RoutePattern::parse("/static/*");
        let params = pattern.match_path("/static/css/style.css").unwrap();
        assert_eq!(params.get("*"), Some(&"css/style.css".to_string()));
    }

    #[test]
    fn query_string_is_ignored() {
        let pattern = RoutePattern::parse("/users/:id");
        let params = pattern.match_path("/users/7?foo=bar").unwrap();
        assert_eq!(params.get("id"), Some(&"7".to_string()));
    }
}
