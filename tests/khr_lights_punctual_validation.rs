//! KHR_lights_punctual per-light property validation.
//!
//! Exercises `validation::validate_khr_lights_punctual`, which enforces
//! the constraints from `docs/3d/gltf/extensions/KHR_lights_punctual.md`:
//! the `type` enum, finite color / intensity, the `range` MUST be > 0 and
//! point/spot-only rule, the spot-cone-angle ordering + `PI / 2` ceiling,
//! the `spot`-required-for-spot / `spot`-forbidden-elsewhere rules, and
//! the per-node light-index bounds.

use oxideav_gltf::json_model::GltfRoot;
use oxideav_gltf::validation::validate_khr_lights_punctual;

fn root_from(json: &str) -> GltfRoot {
    serde_json::from_str(json).expect("fixture parses")
}

/// A document with one root light of the given JSON object, declaring the
/// extension as used.
fn one_light(light_json: &str) -> GltfRoot {
    root_from(&format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "extensionsUsed": ["KHR_lights_punctual"],
            "extensions": {{
                "KHR_lights_punctual": {{ "lights": [ {light_json} ] }}
            }}
        }}"#
    ))
}

#[test]
fn accepts_minimal_directional_point_spot() {
    let root = root_from(
        r#"{
            "asset": { "version": "2.0" },
            "extensionsUsed": ["KHR_lights_punctual"],
            "extensions": {
                "KHR_lights_punctual": { "lights": [
                    { "type": "directional" },
                    { "type": "point" },
                    { "type": "spot", "spot": {} }
                ] }
            }
        }"#,
    );
    validate_khr_lights_punctual(&root).expect("minimal lights are valid");
}

#[test]
fn accepts_full_spot_with_explicit_cones() {
    let root = one_light(
        r#"{ "type": "spot", "color": [1.0, 0.9, 0.8], "intensity": 100.0,
             "range": 25.0,
             "spot": { "innerConeAngle": 0.2, "outerConeAngle": 0.5 } }"#,
    );
    validate_khr_lights_punctual(&root).unwrap();
}

#[test]
fn rejects_unknown_type() {
    let root = one_light(r#"{ "type": "area" }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightType"),
        "got {err}"
    );
}

#[test]
fn rejects_nonfinite_intensity() {
    let root = one_light(r#"{ "type": "point", "intensity": 1e40 }"#);
    // 1e40 is finite as f32? No — it overflows f32 to +inf, so it is
    // rejected for non-finiteness.
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightIntensity"),
        "got {err}"
    );
}

#[test]
fn rejects_negative_intensity() {
    let root = one_light(r#"{ "type": "point", "intensity": -1.0 }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightIntensity"),
        "got {err}"
    );
}

#[test]
fn rejects_range_on_directional_light() {
    let root = one_light(r#"{ "type": "directional", "range": 10.0 }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightRange"),
        "got {err}"
    );
}

#[test]
fn rejects_nonpositive_range() {
    let root = one_light(r#"{ "type": "point", "range": 0.0 }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightRange"),
        "got {err}"
    );
}

#[test]
fn accepts_positive_range_on_point_light() {
    let root = one_light(r#"{ "type": "point", "range": 0.001 }"#);
    validate_khr_lights_punctual(&root).unwrap();
}

#[test]
fn rejects_spot_missing_spot_property() {
    let root = one_light(r#"{ "type": "spot" }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightSpotRequired"),
        "got {err}"
    );
}

#[test]
fn rejects_spot_property_on_point_light() {
    let root = one_light(r#"{ "type": "point", "spot": {} }"#);
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightSpotMisplaced"),
        "got {err}"
    );
}

#[test]
fn rejects_inner_cone_not_less_than_outer() {
    let root = one_light(
        r#"{ "type": "spot",
             "spot": { "innerConeAngle": 0.6, "outerConeAngle": 0.5 } }"#,
    );
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightConeOrder"),
        "got {err}"
    );
}

#[test]
fn rejects_equal_cone_angles() {
    let root = one_light(
        r#"{ "type": "spot",
             "spot": { "innerConeAngle": 0.5, "outerConeAngle": 0.5 } }"#,
    );
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightConeOrder"),
        "got {err}"
    );
}

#[test]
fn rejects_negative_inner_cone() {
    let root = one_light(
        r#"{ "type": "spot",
             "spot": { "innerConeAngle": -0.1, "outerConeAngle": 0.5 } }"#,
    );
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightInnerCone"),
        "got {err}"
    );
}

#[test]
fn rejects_outer_cone_above_half_pi() {
    let root = one_light(
        r#"{ "type": "spot",
             "spot": { "innerConeAngle": 0.1, "outerConeAngle": 1.7 } }"#,
    );
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightOuterCone"),
        "got {err}"
    );
}

#[test]
fn accepts_outer_cone_at_half_pi() {
    // Default inner (0) < outer; outer exactly PI / 2 is allowed.
    let root = one_light(
        r#"{ "type": "spot",
             "spot": { "outerConeAngle": 1.5707963267948966 } }"#,
    );
    validate_khr_lights_punctual(&root).unwrap();
}

#[test]
fn accepts_in_range_node_light_reference() {
    let root = root_from(
        r#"{
            "asset": { "version": "2.0" },
            "extensionsUsed": ["KHR_lights_punctual"],
            "nodes": [
                { "extensions": { "KHR_lights_punctual": { "light": 1 } } }
            ],
            "extensions": {
                "KHR_lights_punctual": { "lights": [
                    { "type": "point" },
                    { "type": "directional" }
                ] }
            }
        }"#,
    );
    validate_khr_lights_punctual(&root).unwrap();
}

#[test]
fn rejects_out_of_range_node_light_reference() {
    let root = root_from(
        r#"{
            "asset": { "version": "2.0" },
            "extensionsUsed": ["KHR_lights_punctual"],
            "nodes": [
                { "extensions": { "KHR_lights_punctual": { "light": 2 } } }
            ],
            "extensions": {
                "KHR_lights_punctual": { "lights": [
                    { "type": "point" }
                ] }
            }
        }"#,
    );
    let err = validate_khr_lights_punctual(&root).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightRef"),
        "got {err}"
    );
}

#[test]
fn full_decode_rejects_bad_spot_cone_order() {
    // The per-light checks run on the decode path via
    // `validate_extension_stack`, so a malformed spot cone is rejected by
    // the public `GltfDecoder::decode` entry too.
    use oxideav_gltf::GltfDecoder;
    use oxideav_mesh3d::Mesh3DDecoder;

    let json = r#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_lights_punctual"],
        "extensions": {
            "KHR_lights_punctual": { "lights": [
                { "type": "spot",
                  "spot": { "innerConeAngle": 1.0, "outerConeAngle": 0.5 } }
            ] }
        }
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json.as_bytes()).unwrap_err();
    assert!(
        err.to_string().contains("ExtensionStackLightConeOrder"),
        "got {err}"
    );
}
