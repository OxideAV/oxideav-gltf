//! Cross-surface integration for the three typed surfaces adopted
//! from the published `oxideav-mesh3d` 0.0.5 model — one document
//! carrying all of them at once:
//!
//! * `Option`-shaped sampler filters (glTF 2.0 §3.8.4.1 — no filter
//!   default) with non-default wrap modes,
//! * `TextureRef::transform` (`KHR_texture_transform`) with a
//!   `texCoord` override,
//! * `Node::weights` (§5.25.9) alongside `mesh.weights` and an
//!   animated `MorphWeights` channel — the full §3.7.4
//!   *animation > node > mesh* chain on the wire.
//!
//! Both container flavours are exercised, and the second encode must
//! be semantically identical to the first — same JSON document value
//! and same BIN payload — so the decode → encode cycle is a fixed
//! point and none of the three surfaces drifts or accretes. (Byte
//! parity is not asserted because the `attributes` map serialises in
//! hash order, which is not stable across encodes.)

use oxideav_gltf::{GltfDecoder, GltfEncoder, OutputFlavour};
use oxideav_mesh3d::{
    Animation, AnimationChannel, AnimationProperty, AnimationSampler, AnimationTarget,
    AnimationValues, Interpolation, MagFilter, Material, Mesh, Mesh3DDecoder, Mesh3DEncoder,
    MinFilter, MorphTarget, Node, NodeId, Primitive, Sampler, Scene3D, Texture, TextureRef,
    TextureTransform, Topology, WrapMode,
};

fn build_scene() -> Scene3D {
    let mut scene = Scene3D::new();

    // Texture with a fully-explicit non-default sampler.
    let mut tex = Texture::from_encoded("image/png".to_owned(), vec![0xA5u8; 16]);
    tex.sampler = Sampler::default_sampler()
        .with_mag_filter(MagFilter::Nearest)
        .with_min_filter(MinFilter::NearestMipLinear)
        .with_wrap(WrapMode::ClampToEdge, WrapMode::MirroredRepeat);
    let tex_id = scene.add_texture(tex);
    // Second texture staying in the samplerless default state.
    let plain_id = scene.add_texture(Texture::from_encoded(
        "image/png".to_owned(),
        vec![0x5Au8; 16],
    ));

    // Material: base colour texture carries a transform with a
    // texCoord override onto the second UV set; emissive samples the
    // default-state texture without a transform.
    let mut mat = Material::new();
    mat.base_color_texture = Some(
        TextureRef::new(tex_id).with_transform(
            TextureTransform::new()
                .with_offset([0.25, 0.0])
                .with_rotation(0.5)
                .with_scale([2.0, 2.0])
                .with_uv_set(1),
        ),
    );
    mat.emissive_texture = Some(TextureRef::new(plain_id));
    mat.emissive_factor = [1.0, 1.0, 1.0];
    let mat_id = scene.add_material(mat);

    // Morphed mesh with two UV sets (the transform override targets
    // set 1) and one POSITION morph target.
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    prim.uvs = vec![
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
        vec![[0.5, 0.5], [0.75, 0.5], [0.5, 0.75]],
    ];
    prim.material = Some(mat_id);
    prim.targets = vec![MorphTarget::with_deltas(
        Some(vec![[0.1, 0.0, 0.0], [0.1, 0.0, 0.0], [0.1, 0.0, 0.0]]),
        None,
        None,
    )];
    let mut mesh = Mesh::new(Some("morphed".to_owned()));
    mesh.primitives.push(prim);
    mesh.weights = vec![0.5];
    let mesh_id = scene.add_mesh(mesh);

    // Two instances of the same mesh: one overriding the default
    // blend state, one inheriting it.
    let n0 = scene.add_node(Node::new().with_mesh(mesh_id).with_weights([0.25]));
    let n1 = scene.add_node(Node::new().with_mesh(mesh_id));
    scene.add_root(n0);
    scene.add_root(n1);

    // Animated MorphWeights channel on the overriding node — the top
    // rung of the §3.7.4 chain.
    let mut anim = Animation::new(Some("blend".to_owned()));
    anim.channels.push(AnimationChannel {
        target: AnimationTarget {
            node: n0,
            property: AnimationProperty::MorphWeights,
        },
        sampler: AnimationSampler {
            keyframes: vec![0.0, 1.0],
            values: AnimationValues::Scalar(vec![0.0, 1.0]),
            interpolation: Interpolation::Linear,
        },
    });
    scene.animations.push(anim);

    scene
}

fn assert_adopted_surfaces(scene: &Scene3D) {
    // Sampler: explicit filters + wraps on texture 0, the untouched
    // default state on texture 1.
    let s0 = scene.textures[0].sampler;
    assert_eq!(s0.mag_filter, Some(MagFilter::Nearest));
    assert_eq!(s0.min_filter, Some(MinFilter::NearestMipLinear));
    assert_eq!(s0.wrap_s, WrapMode::ClampToEdge);
    assert_eq!(s0.wrap_t, WrapMode::MirroredRepeat);
    assert_eq!(scene.textures[1].sampler, Sampler::default_sampler());

    // Texture transform: typed, with the texCoord override resolving.
    let r = scene.materials[0].base_color_texture.unwrap();
    let tt = r.transform.expect("typed transform survives");
    assert_eq!(tt.offset, [0.25, 0.0]);
    assert!((tt.rotation - 0.5).abs() < 1e-6);
    assert_eq!(tt.scale, [2.0, 2.0]);
    assert_eq!(tt.uv_set, Some(1));
    assert_eq!(r.effective_uv_set(), 1);
    assert_eq!(
        scene.materials[0].emissive_texture.unwrap().transform,
        None,
        "the transform-free slot stays undeclared"
    );

    // Node weights: per-instance override vs inherited mesh default.
    assert_eq!(scene.nodes[0].weights, vec![0.25]);
    assert!(scene.nodes[1].weights.is_empty());
    assert_eq!(scene.meshes[0].weights, vec![0.5]);
    assert_eq!(
        scene.effective_morph_weights(NodeId(0)),
        Some(&[0.25f32][..])
    );
    assert_eq!(
        scene.effective_morph_weights(NodeId(1)),
        Some(&[0.5f32][..])
    );

    // The animated rung is still on the document.
    let ch = &scene.animations[0].channels[0];
    assert_eq!(ch.target.property, AnimationProperty::MorphWeights);
    assert_eq!(ch.sampler.values, AnimationValues::Scalar(vec![0.0, 1.0]));
}

/// Split an encoded document into (JSON value, BIN payload). For the
/// GLB flavour the two chunks are read per spec §4; for the JSON
/// flavour the whole document is the value (the buffer rides inside
/// it as a `data:` URI, so it is covered by the value comparison).
fn semantic_parts(flavour: OutputFlavour, bytes: &[u8]) -> (serde_json::Value, Vec<u8>) {
    match flavour {
        OutputFlavour::Glb => {
            assert_eq!(&bytes[0..4], b"glTF");
            let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
            let json: serde_json::Value =
                serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
            let bin = bytes[20 + json_len..].to_vec();
            (json, bin)
        }
        OutputFlavour::JsonEmbedded => (serde_json::from_slice(bytes).unwrap(), Vec::new()),
    }
}

fn roundtrip(flavour: OutputFlavour) {
    let scene = build_scene();
    let mut enc = GltfEncoder::with_output(flavour);
    let bytes = enc.encode(&scene).unwrap();

    let decoded = GltfDecoder::new().decode(&bytes).unwrap();
    assert_adopted_surfaces(&decoded);

    // Fixed point: re-encoding the decoded scene reproduces the same
    // document — none of the adopted surfaces drifts (e.g. a
    // manufactured filter, a dropped identity transform, a weights
    // vector migrating between node and mesh).
    let bytes2 = enc.encode(&decoded).unwrap();
    let (json1, bin1) = semantic_parts(flavour, &bytes);
    let (json2, bin2) = semantic_parts(flavour, &bytes2);
    assert_eq!(json1, json2, "decode → encode must be a JSON fixed point");
    assert_eq!(bin1, bin2, "decode → encode must be a BIN fixed point");

    // And the surfaces survive a second full cycle.
    let decoded2 = GltfDecoder::new().decode(&bytes2).unwrap();
    assert_adopted_surfaces(&decoded2);
}

#[test]
fn all_adopted_surfaces_roundtrip_via_glb() {
    roundtrip(OutputFlavour::Glb);
}

#[test]
fn all_adopted_surfaces_roundtrip_via_json() {
    roundtrip(OutputFlavour::JsonEmbedded);
}
