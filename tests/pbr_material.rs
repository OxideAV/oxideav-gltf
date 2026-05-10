//! Materials with every PBR field populated round-trip without loss.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    AlphaMode, ImageData, Material, MaterialId, Mesh, Mesh3DDecoder, Mesh3DEncoder, Node,
    Primitive, Sampler, Scene3D, Texture, TextureId, TextureRef, Topology,
};

fn dummy_texture(name: &str, mime: &str) -> Texture {
    Texture::from_encoded(mime.to_owned(), vec![0xFFu8; 32]).clone_named(name)
}

trait Named {
    fn clone_named(self, name: &str) -> Self;
}

impl Named for Texture {
    fn clone_named(mut self, name: &str) -> Self {
        self.name = Some(name.to_owned());
        self
    }
}

#[test]
fn full_pbr_roundtrip() {
    let mut scene = Scene3D::new();
    let base_tex = scene.add_texture(dummy_texture("base", "image/png"));
    let mr_tex = scene.add_texture(dummy_texture("mr", "image/png"));
    let nrm_tex = scene.add_texture(dummy_texture("nrm", "image/png"));
    let occ_tex = scene.add_texture(dummy_texture("occ", "image/png"));
    let emi_tex = scene.add_texture(dummy_texture("emi", "image/png"));

    let mut mat = Material::new();
    mat.name = Some("full_pbr".into());
    mat.base_color = [0.2, 0.4, 0.6, 0.8];
    mat.base_color_texture = Some(TextureRef::new(base_tex));
    mat.metallic = 0.7;
    mat.roughness = 0.3;
    mat.metallic_roughness_texture = Some(TextureRef::new(mr_tex));
    mat.normal_texture = Some(TextureRef::new(nrm_tex));
    mat.normal_scale = 0.5;
    mat.occlusion_texture = Some(TextureRef::new(occ_tex));
    mat.occlusion_strength = 0.6;
    mat.emissive_factor = [0.1, 0.2, 0.3];
    mat.emissive_texture = Some(TextureRef::new(emi_tex));
    mat.alpha_mode = AlphaMode::Mask { cutoff: 0.25 };
    mat.double_sided = true;
    let mid = scene.add_material(mat);

    let mut prim = Primitive::new(Topology::Triangles);
    prim.positions = vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    prim.uvs = vec![vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]];
    prim.material = Some(mid);
    let mut mesh = Mesh::new(Some("m".to_owned()));
    mesh.primitives.push(prim);
    let meshid = scene.add_mesh(mesh);
    let nid = scene.add_node(Node::new().with_mesh(meshid));
    scene.add_root(nid);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();
    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();

    let m = &decoded.materials[0];
    assert_eq!(m.name.as_deref(), Some("full_pbr"));
    assert_eq!(m.base_color, [0.2, 0.4, 0.6, 0.8]);
    assert_eq!(m.metallic, 0.7);
    assert_eq!(m.roughness, 0.3);
    assert_eq!(m.normal_scale, 0.5);
    assert_eq!(m.occlusion_strength, 0.6);
    assert_eq!(m.emissive_factor, [0.1, 0.2, 0.3]);
    assert_eq!(m.alpha_mode, AlphaMode::Mask { cutoff: 0.25 });
    assert!(m.double_sided);
    assert!(m.base_color_texture.is_some());
    assert!(m.metallic_roughness_texture.is_some());
    assert!(m.normal_texture.is_some());
    assert!(m.occlusion_texture.is_some());
    assert!(m.emissive_texture.is_some());

    // Texture image bytes survived the BIN-chunk round trip via BufferViewAsset.
    assert_eq!(decoded.textures.len(), 5);
    for tex in &decoded.textures {
        match &tex.image {
            ImageData::Source(src) => {
                use std::io::Read;
                let mut got = Vec::new();
                src.open().unwrap().read_to_end(&mut got).unwrap();
                assert_eq!(got, vec![0xFFu8; 32]);
            }
            other => panic!("unexpected ImageData {other:?}"),
        }
    }

    // Suppress "unused" warnings for the imports we kept around for clarity.
    let _ = (Sampler::default_sampler(), MaterialId(0), TextureId(0));
}
