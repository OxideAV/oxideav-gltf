//! KHR_node_visibility extension — Boolean `visible` flag on a node
//! per `docs/3d/gltf/extensions/KHR_node_visibility.md`. The decoder
//! surfaces the value through `oxideav_mesh3d::Node::extras
//! ["KHR_node_visibility"] = Bool(...)`; the encoder lifts it back into
//! the typed `KHR_node_visibility` extension object on write and
//! declares the extension in `extensionsUsed`. The §3.12 validator
//! rejects a node carrying the extension object without the
//! corresponding `extensionsUsed` entry.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{Mesh3DDecoder, Mesh3DEncoder, Node, Scene3D};
use serde_json::Value;

#[test]
fn node_visibility_false_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut node = Node::new();
    node.extras
        .insert("KHR_node_visibility".to_owned(), Value::Bool(false));
    scene.add_node(node);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.nodes.len(), 1);
    let dn = &decoded.nodes[0];
    assert_eq!(
        dn.extras.get("KHR_node_visibility"),
        Some(&Value::Bool(false)),
        "visibility flag survives round-trip"
    );
}

#[test]
fn node_visibility_true_roundtrips_via_glb() {
    let mut scene = Scene3D::new();
    let mut node = Node::new();
    node.extras
        .insert("KHR_node_visibility".to_owned(), Value::Bool(true));
    scene.add_node(node);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&glb).unwrap();

    assert_eq!(decoded.nodes.len(), 1);
    let dn = &decoded.nodes[0];
    assert_eq!(
        dn.extras.get("KHR_node_visibility"),
        Some(&Value::Bool(true)),
        "explicit visible=true survives round-trip"
    );
}

#[test]
fn node_visibility_emits_extensions_used_on_encode() {
    let mut scene = Scene3D::new();
    let mut node = Node::new();
    node.extras
        .insert("KHR_node_visibility".to_owned(), Value::Bool(false));
    scene.add_node(node);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(
        raw.contains("\"extensionsUsed\""),
        "extensionsUsed must be emitted, got: {raw}"
    );
    assert!(
        raw.contains("\"KHR_node_visibility\""),
        "KHR_node_visibility must appear in JSON, got: {raw}"
    );
    // Per the spec §Extending Nodes the extension object carries a
    // single optional boolean `visible`; here we explicitly set it to
    // false so the JSON output should contain the typed object form.
    assert!(
        raw.contains("\"visible\":false"),
        "visible=false must round-trip into the JSON object, got: {raw}"
    );
}

#[test]
fn node_without_visibility_does_not_emit_extension() {
    let mut scene = Scene3D::new();
    scene.add_node(Node::new());

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let raw_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&raw_bytes).unwrap();
    assert!(
        !raw.contains("KHR_node_visibility"),
        "extension must NOT appear when no node sets the flag, got: {raw}"
    );
}

#[test]
fn node_visibility_data_block_without_extensions_used_is_rejected() {
    // Hand-build JSON with a per-node KHR_node_visibility block but no
    // `extensionsUsed` declaration — spec §3.12 violation. The
    // validator must reject with `ExtensionStackUsedNotDeclared`.
    let json = br#"{
        "asset": { "version": "2.0" },
        "nodes": [
            {
                "extensions": { "KHR_node_visibility": { "visible": false } }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let err = dec.decode(json).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("ExtensionStackUsedNotDeclared") && msg.contains("KHR_node_visibility"),
        "expected ExtensionStackUsedNotDeclared for KHR_node_visibility, got {msg}"
    );
}

#[test]
fn node_visibility_data_block_with_extensions_used_decodes() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_node_visibility"],
        "nodes": [
            {
                "extensions": { "KHR_node_visibility": { "visible": false } }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let scene = dec
        .decode(json)
        .expect("declared extension decodes cleanly");
    assert_eq!(scene.nodes.len(), 1);
    assert_eq!(
        scene.nodes[0].extras.get("KHR_node_visibility"),
        Some(&Value::Bool(false)),
        "visibility flag must be surfaced through Node::extras"
    );
}

#[test]
fn node_visibility_bare_object_resolves_to_spec_default_true() {
    // Per docs/3d/gltf/extensions/KHR_node_visibility.md §Extending
    // Nodes the `visible` field is optional with a default of `true`.
    // A bare `{}` object MUST resolve to that default — the decoder
    // surfaces `true` so downstream consumers see a defined value.
    let json = br#"{
        "asset": { "version": "2.0" },
        "extensionsUsed": ["KHR_node_visibility"],
        "nodes": [
            {
                "extensions": { "KHR_node_visibility": {} }
            }
        ]
    }"#;
    let mut dec = GltfDecoder::new();
    let scene = dec.decode(json).unwrap();
    assert_eq!(scene.nodes.len(), 1);
    assert_eq!(
        scene.nodes[0].extras.get("KHR_node_visibility"),
        Some(&Value::Bool(true)),
        "bare {{}} resolves to spec default visible=true"
    );
}

#[test]
fn node_visibility_coexists_with_lights_punctual() {
    // The two per-node extensions must coexist on the same node — the
    // encoder emits both inside the per-node `extensions` block and
    // declares both in `extensionsUsed`.
    use oxideav_mesh3d::Light;
    let mut scene = Scene3D::new();
    let light_id = scene.add_light(Light::Point {
        color: [1.0, 1.0, 1.0],
        intensity: 100.0,
        range: None,
    });
    let mut node = Node::new();
    node.light = Some(light_id);
    node.extras
        .insert("KHR_node_visibility".to_owned(), Value::Bool(false));
    scene.add_node(node);

    let glb = GltfEncoder::new().encode(&scene).unwrap();
    let json_bytes = extract_json_chunk(&glb);
    let raw = std::str::from_utf8(&json_bytes).unwrap();
    assert!(raw.contains("\"KHR_lights_punctual\""));
    assert!(raw.contains("\"KHR_node_visibility\""));

    let decoded = GltfDecoder::new().decode(&glb).unwrap();
    assert_eq!(decoded.nodes.len(), 1);
    let dn = &decoded.nodes[0];
    assert!(dn.light.is_some(), "light reference survives round-trip");
    assert_eq!(
        dn.extras.get("KHR_node_visibility"),
        Some(&Value::Bool(false)),
        "visibility flag survives round-trip alongside light"
    );
}

/// Walk the `.glb` container and return its JSON chunk's payload bytes.
/// Matches the layout from glTF 2.0 spec §4 (12-byte file header,
/// then chunks of `length:u32, type:u32, payload`).
fn extract_json_chunk(glb: &[u8]) -> Vec<u8> {
    assert_eq!(&glb[0..4], b"glTF", "magic");
    let chunk_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let chunk_type = &glb[16..20];
    assert_eq!(chunk_type, b"JSON", "first chunk type must be JSON");
    glb[20..20 + chunk_len].to_vec()
}
