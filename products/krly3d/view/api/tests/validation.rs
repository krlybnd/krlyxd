#![allow(clippy::unwrap_used)]
use krly3d_view_api::{ApiVersion, Camera, LengthUnit, RenderCall, RenderInput, RequestId};

fn call() -> RenderCall {
    RenderCall::new(RequestId::new("image-1"), RenderInput::new("part"))
}

#[test]
fn version_and_identity_errors_are_distinct_from_geometry_inputs() {
    let mut request = call();
    request.api = ApiVersion { major: 2, minor: 0 };
    assert_eq!(request.validate().unwrap_err().code.as_str(), "unsupported_version");
    request = call();
    request.request_id = RequestId::new("");
    assert_eq!(request.validate().unwrap_err().code.as_str(), "input");
    request = call();
    request.input.node.clear();
    assert_eq!(request.validate().unwrap_err().code.as_str(), "input");
}

#[test]
fn invalid_image_sizes_are_rejected_before_engine_allocation() {
    for (width, height) in [(0, 1), (1, 0), (2049, 1), (1, 2049), (u32::MAX, u32::MAX)] {
        let mut request = call();
        request.input.width = width;
        request.input.height = height;
        assert_eq!(request.validate().unwrap_err().code.as_str(), "input");
    }
    for size in [1, 2048] {
        let mut request = call();
        request.input.width = size;
        request.input.height = size;
        assert!(request.validate().is_ok());
    }
}

#[test]
fn tolerance_rejects_nonfinite_values_and_unconverted_physical_units() {
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut request = call();
        request.input.tolerance.value = value;
        assert_eq!(request.validate().unwrap_err().code.as_str(), "input");
    }
    let mut request = call();
    request.input.tolerance.unit = LengthUnit::Metre;
    assert_eq!(request.validate().unwrap_err().code.as_str(), "input");
}

#[test]
fn invalid_camera_scalars_and_empty_directions_do_not_reach_a_renderer() {
    for direction in [[0.0; 3], [f64::NAN, 0.0, 1.0], [1.0, f64::INFINITY, 0.0]] {
        for camera in [
            Camera::Orthographic { toward: direction, up: [0.0, 0.0, 1.0] },
            Camera::Orthographic { toward: [0.0, 1.0, 0.0], up: direction },
        ] {
            let mut request = call();
            request.input.camera = camera;
            assert!(request.validate().is_err());
        }
    }
    for (eye, focal, centre) in [
        ([f64::NAN, 0.0, 0.0], 100.0, None),
        ([0.0; 3], 0.0, None),
        ([0.0; 3], -1.0, None),
        ([0.0; 3], f64::INFINITY, None),
        ([0.0; 3], f64::from_bits(1), None),
        ([0.0; 3], 100.0, Some([0.0, f64::NAN])),
    ] {
        let mut request = call();
        request.input.camera =
            Camera::Pinhole { eye, toward: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], focal, centre };
        assert!(request.validate().is_err());
    }
}

#[test]
fn nonunit_camera_directions_and_offcentre_pinhole_are_valid_inputs() {
    for camera in [
        Camera::Orthographic { toward: [0.0, 4.0, 0.0], up: [0.0, 0.0, 2.0] },
        Camera::Pinhole {
            eye: [0.0, -20.0, 3.0],
            toward: [0.0, 4.0, 0.0],
            up: [0.0, 0.0, 2.0],
            focal: 80.0,
            centre: Some([-4.0, 24.0]),
        },
    ] {
        let mut request = call();
        request.input.camera = camera;
        assert!(request.validate().is_ok());
    }
}
