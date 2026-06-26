//! Deterministic property/fuzz coverage for the write-side
//! `KHR_meshopt_compression` path: many pseudo-randomly generated meshes
//! are encoded with compression enabled, decoded back, and checked to
//! reproduce the original attribute + index data byte-for-byte.
//!
//! Uses a fixed-seed LCG so failures are reproducible (no external
//! `rand`/`proptest` dependency, matching the crate's test conventions).

use oxideav_gltf::{GltfDecoder, GltfEncoder, OutputFlavour};
use oxideav_mesh3d::{
    Indices, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node, Primitive, Scene3D, Topology,
};

/// Minimal linear-congruential PRNG (Numerical Recipes constants).
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    /// Uniform-ish f32 in `[-range, range]`.
    fn f32_sym(&mut self, range: f32) -> f32 {
        let u = (self.next() >> 8) as f32 / (1u32 << 24) as f32; // [0,1)
        (u * 2.0 - 1.0) * range
    }
    fn range(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// Build a random indexed triangle mesh: `vcount` vertices, `tris`
/// triangles, with POSITION + optional NORMAL.
fn random_mesh(rng: &mut Lcg, vcount: u32, tris: u32, with_normals: bool) -> Scene3D {
    let positions: Vec<[f32; 3]> = (0..vcount)
        .map(|_| [rng.f32_sym(100.0), rng.f32_sym(100.0), rng.f32_sym(100.0)])
        .collect();
    let normals: Option<Vec<[f32; 3]>> = if with_normals {
        Some(
            (0..vcount)
                .map(|_| {
                    let v = [rng.f32_sym(1.0), rng.f32_sym(1.0), rng.f32_sym(1.0)];
                    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
                    [v[0] / len, v[1] / len, v[2] / len]
                })
                .collect(),
        )
    } else {
        None
    };
    let indices: Vec<u32> = (0..tris * 3).map(|_| rng.range(vcount)).collect();

    let mut scene = Scene3D::new();
    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = positions;
    prim.normals = normals;
    prim.indices = Some(Indices::U32(indices));
    let mut mesh = Mesh::new(Some("fuzz".to_owned()));
    mesh.primitives.push(prim);
    let mid = scene.add_mesh(mesh);
    let node = Node::new().with_mesh(mid);
    let nid = scene.add_node(node);
    scene.add_root(nid);
    scene
}

/// Encode `scene` with meshopt compression on, decode it back, and
/// assert the primitive's positions / normals / triangle indices match
/// what a plain (uncompressed) encode→decode produces. Comparing against
/// the uncompressed round-trip (rather than the source scene) isolates
/// the meshopt codec from unrelated encode normalisation (e.g. index
/// component narrowing).
fn assert_meshopt_roundtrip(scene: &Scene3D, flavour: OutputFlavour) {
    let plain = {
        let mut enc = GltfEncoder::with_output(flavour);
        let bytes = enc.encode(scene).expect("plain encode");
        let mut dec = GltfDecoder::new();
        dec.decode(&bytes).expect("plain decode")
    };
    let compressed = {
        let mut enc = GltfEncoder::with_output(flavour).with_meshopt_compression(true);
        let bytes = enc.encode(scene).expect("meshopt encode");
        let mut dec = GltfDecoder::new();
        dec.decode(&bytes).expect("meshopt decode")
    };

    let pp = &plain.meshes[0].primitives[0];
    let cp = &compressed.meshes[0].primitives[0];
    assert_eq!(cp.positions, pp.positions, "positions diverged");
    assert_eq!(cp.normals, pp.normals, "normals diverged");
    assert_eq!(
        cp.triangle_indices(),
        pp.triangle_indices(),
        "indices diverged"
    );
}

#[test]
fn fuzz_meshopt_json_roundtrip() {
    let mut rng = Lcg(0xC0FF_EE42);
    for i in 0..64 {
        // Keep vertex counts above 255 sometimes so the index accessor
        // stays u16/u32 (the compressible widths), and vary triangle
        // counts to span multiple attribute groups / index blocks.
        let vcount = 16 + rng.range(900);
        let tris = 1 + rng.range(400);
        let with_normals = i % 2 == 0;
        let scene = random_mesh(&mut rng, vcount, tris, with_normals);
        assert_meshopt_roundtrip(&scene, OutputFlavour::JsonEmbedded);
    }
}

#[test]
fn fuzz_meshopt_glb_roundtrip() {
    let mut rng = Lcg(0x1357_9BDF);
    for _ in 0..32 {
        let vcount = 300 + rng.range(700);
        let tris = 1 + rng.range(300);
        let scene = random_mesh(&mut rng, vcount, tris, true);
        assert_meshopt_roundtrip(&scene, OutputFlavour::Glb);
    }
}

#[test]
fn fuzz_meshopt_large_attribute_blocks() {
    // A single large primitive forces the ATTRIBUTES encoder across
    // multiple 16-element groups and (for big strides) multiple blocks.
    let mut rng = Lcg(0xABCD_1234);
    let scene = random_mesh(&mut rng, 4000, 1200, true);
    assert_meshopt_roundtrip(&scene, OutputFlavour::JsonEmbedded);
}
