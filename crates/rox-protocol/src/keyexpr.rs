//! KeyExpr wildcard matching support.
//!
//! Supports zenoh-style wildcard patterns:
//! - `*` matches a single level
//! - `**` matches zero or more levels

use crate::contracts::KeyExpr;

/// Wildcard matcher for KeyExpr patterns.
pub struct KeyExprMatcher;

impl KeyExprMatcher {
    /// Check if a key matches a wildcard pattern.
    ///
    /// Pattern syntax:
    /// - `robot-01/lidar/scan` — exact match
    /// - `robot-01/*/scan` — `*` matches exactly one level
    /// - `robot-01/**` — `**` matches zero or more levels
    pub fn matches(key: &KeyExpr, pattern: &str) -> bool {
        let key_parts: Vec<&str> = key.as_str().split('/').collect();
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        Self::match_parts(&key_parts, &pattern_parts)
    }

    fn match_parts(key: &[&str], pattern: &[&str]) -> bool {
        if pattern.is_empty() {
            return key.is_empty();
        }

        if pattern[0] == "**" {
            // `**` matches zero or more levels
            if Self::match_parts(key, &pattern[1..]) {
                return true;
            }
            if !key.is_empty() {
                return Self::match_parts(&key[1..], pattern);
            }
            return false;
        }

        if key.is_empty() {
            return false;
        }

        if pattern[0] == "*" || pattern[0] == key[0] {
            return Self::match_parts(&key[1..], &pattern[1..]);
        }

        false
    }
}

impl KeyExpr {
    /// Check if this key matches a wildcard pattern.
    pub fn matches(&self, pattern: &str) -> bool {
        KeyExprMatcher::matches(self, pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let key = KeyExpr::new("robot-01", "lidar", "scan");
        assert!(key.matches("robot-01/lidar/scan"));
        assert!(!key.matches("robot-01/lidar/raw"));
    }

    #[test]
    fn test_single_wildcard() {
        let key = KeyExpr::new("robot-01", "lidar", "scan");
        assert!(key.matches("robot-01/*/scan"));
        assert!(key.matches("*/lidar/scan"));
        assert!(key.matches("robot-01/lidar/*"));
        assert!(!key.matches("robot-01/*"));
    }

    #[test]
    fn test_double_wildcard() {
        let key = KeyExpr::new("robot-01", "lidar", "scan");
        assert!(key.matches("robot-01/**"));
        assert!(key.matches("**"));
        assert!(key.matches("robot-01/lidar/**"));
    }

    #[test]
    fn test_global_topic() {
        let key = KeyExpr::global("map/occupancy");
        assert!(key.matches("_global/map/occupancy"));
        assert!(key.matches("_global/**"));
        assert!(key.matches("_global/*/occupancy"));
    }

    #[test]
    fn test_robot_id_extraction() {
        let key = KeyExpr::new("robot-01", "lidar", "scan");
        assert_eq!(key.robot_id(), Some("robot-01"));

        let global = KeyExpr::global("map");
        assert_eq!(global.robot_id(), None);
    }
}
