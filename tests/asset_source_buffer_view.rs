//! `BufferViewAsset::open()` reads back exactly the texture bytes
//! that were packed into the `.glb` BIN chunk.

use oxideav_gltf::{GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{ImageData, Mesh3DDecoder, Mesh3DEncoder, Scene3D, Texture};
use std::io::Read;

#[test]
fn buffer_view_asset_reads_correct_bytes() {
    let mut scene = Scene3D::new();
    let payload = (0u8..=255).cycle().take(257).collect::<Vec<_>>();
    let mut tex = Texture::from_encoded("image/png".to_owned(), payload.clone());
    tex.name = Some("blob".into());
    scene.add_texture(tex);

    let mut enc = GltfEncoder::new();
    let glb = enc.encode(&scene).unwrap();

    let mut dec = GltfDecoder::new();
    let decoded = dec.decode(&glb).unwrap();
    assert_eq!(decoded.textures.len(), 1);
    let src = match &decoded.textures[0].image {
        ImageData::Source(s) => s.clone(),
        other => panic!("expected ImageData::Source, got {other:?}"),
    };
    assert_eq!(src.size_hint(), Some(payload.len() as u64));
    assert_eq!(src.mime(), Some("image/png"));
    let mut got = Vec::new();
    src.open().unwrap().read_to_end(&mut got).unwrap();
    assert_eq!(
        got, payload,
        "texture bytes mismatch through BufferViewAsset"
    );
}
