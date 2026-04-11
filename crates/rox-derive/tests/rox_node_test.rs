//! Integration tests for `#[rox_node]` proc-macro.

use rox_derive::rox_node;

#[rox_node]
struct LidarDriver {
    #[rox_sub("sensor/raw")]
    input: Vec<u8>,

    #[rox_pub("lidar/scan")]
    output: Vec<u8>,
}

#[rox_node]
struct VoxelFilter {
    #[rox_sub("lidar/scan")]
    input: Vec<u8>,

    #[rox_pub("lidar/filtered")]
    output: Vec<u8>,

    #[rox_param(default = 0.1)]
    voxel_size: f32,
}

#[rox_node]
struct EmptyNode {}

#[test]
fn test_rox_name() {
    assert_eq!(LidarDriver::rox_name(), "LidarDriver");
    assert_eq!(VoxelFilter::rox_name(), "VoxelFilter");
    assert_eq!(EmptyNode::rox_name(), "EmptyNode");
}

#[test]
fn test_subscriptions() {
    assert_eq!(LidarDriver::rox_subscriptions(), &["sensor/raw"]);
    assert_eq!(VoxelFilter::rox_subscriptions(), &["lidar/scan"]);
    assert_eq!(EmptyNode::rox_subscriptions(), &[] as &[&str]);
}

#[test]
fn test_publications() {
    assert_eq!(LidarDriver::rox_publications(), &["lidar/scan"]);
    assert_eq!(VoxelFilter::rox_publications(), &["lidar/filtered"]);
    assert_eq!(EmptyNode::rox_publications(), &[] as &[&str]);
}

#[rox_node]
struct MultiIO {
    #[rox_sub("camera/rgb")]
    camera_input: Vec<u8>,

    #[rox_sub("lidar/scan")]
    lidar_input: Vec<u8>,

    #[rox_pub("fusion/objects")]
    objects: Vec<u8>,

    #[rox_pub("fusion/debug")]
    debug_output: Vec<u8>,
}

#[test]
fn test_multiple_io() {
    assert_eq!(MultiIO::rox_subscriptions(), &["camera/rgb", "lidar/scan"]);
    assert_eq!(
        MultiIO::rox_publications(),
        &["fusion/objects", "fusion/debug"]
    );
}
