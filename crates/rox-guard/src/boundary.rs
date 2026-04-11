//! Safety boundary definitions — physical limits for actuator commands.

use serde::{Deserialize, Serialize};

/// Geographic fence boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeoFence {
    pub min_x: f64,
    pub max_x: f64,
    pub min_y: f64,
    pub max_y: f64,
}

impl GeoFence {
    /// Check if a position is within the fence.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Clamp a position to the fence boundaries.
    pub fn clamp(&self, x: f64, y: f64) -> (f64, f64) {
        (
            x.clamp(self.min_x, self.max_x),
            y.clamp(self.min_y, self.max_y),
        )
    }
}

/// Safety boundary for a specific actuator node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SafetyBoundary {
    pub node_id: String,
    pub max_velocity: Option<f64>,
    pub max_acceleration: Option<f64>,
    pub max_torque: Option<f64>,
    pub geofence: Option<GeoFence>,
}

impl SafetyBoundary {
    /// Check if a velocity is within limits.
    pub fn check_velocity(&self, velocity: f64) -> BoundaryCheckResult {
        if let Some(max) = self.max_velocity {
            if velocity.abs() > max {
                return BoundaryCheckResult::Exceeded {
                    field: "velocity".to_string(),
                    value: velocity,
                    limit: max,
                    clamped: velocity.signum() * max,
                };
            }
        }
        BoundaryCheckResult::Ok
    }

    /// Check if acceleration is within limits.
    pub fn check_acceleration(&self, accel: f64) -> BoundaryCheckResult {
        if let Some(max) = self.max_acceleration {
            if accel.abs() > max {
                return BoundaryCheckResult::Exceeded {
                    field: "acceleration".to_string(),
                    value: accel,
                    limit: max,
                    clamped: accel.signum() * max,
                };
            }
        }
        BoundaryCheckResult::Ok
    }

    /// Check if a position is within geofence.
    pub fn check_position(&self, x: f64, y: f64) -> BoundaryCheckResult {
        if let Some(fence) = &self.geofence {
            if !fence.contains(x, y) {
                let (_cx, _cy) = fence.clamp(x, y);
                return BoundaryCheckResult::Exceeded {
                    field: format!("geofence(x={x:.2},y={y:.2})"),
                    value: 0.0,
                    limit: 0.0,
                    clamped: 0.0,
                };
            }
        }
        BoundaryCheckResult::Ok
    }
}

/// Result of a boundary check.
#[derive(Clone, Debug)]
pub enum BoundaryCheckResult {
    Ok,
    Exceeded {
        field: String,
        value: f64,
        limit: f64,
        clamped: f64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geofence() {
        let fence = GeoFence {
            min_x: -10.0,
            max_x: 10.0,
            min_y: -10.0,
            max_y: 10.0,
        };

        assert!(fence.contains(0.0, 0.0));
        assert!(fence.contains(10.0, 10.0));
        assert!(!fence.contains(11.0, 0.0));
        assert!(!fence.contains(0.0, -11.0));
    }

    #[test]
    fn test_velocity_boundary() {
        let boundary = SafetyBoundary {
            node_id: "motor".to_string(),
            max_velocity: Some(2.0),
            max_acceleration: Some(5.0),
            max_torque: None,
            geofence: None,
        };

        assert!(matches!(
            boundary.check_velocity(1.5),
            BoundaryCheckResult::Ok
        ));
        assert!(matches!(
            boundary.check_velocity(3.0),
            BoundaryCheckResult::Exceeded { .. }
        ));
    }
}
