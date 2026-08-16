//! Sampler state round-trip per glTF 2.0 §3.8.4.1 + §5.26.
//!
//! The two filters (`magFilter` / `minFilter`) have NO spec default —
//! an undefined filter leaves the choice to the runtime — so the typed
//! `oxideav_mesh3d::Sampler` keeps them `Option`-shaped and this crate
//! must round-trip "undefined" distinguishably from every explicit
//! choice (previously an absent filter was silently coerced to
//! LINEAR / trilinear on decode and re-emitted as an explicit value).
//! The wrap modes DO default (REPEAT, §5.26.3 / §5.26.4), so REPEAT is
//! omitted on the wire and everything else is explicit.

use oxideav_gltf::{json_encoder, GltfDecoder, GltfEncoder};
use oxideav_mesh3d::{
    MagFilter, Mesh3DDecoder, Mesh3DEncoder, MinFilter, Sampler, Scene3D, Texture, WrapMode,
};
use serde_json::Value;

fn doc_with_sampler(sampler_json: &str) -> Vec<u8> {
    format!(
        r#"{{
            "asset": {{ "version": "2.0" }},
            "images": [{{ "uri": "data:image/png;base64,AAAA" }}],
            "samplers": [{sampler_json}],
            "textures": [{{ "source": 0, "sampler": 0 }}]
        }}"#
    )
    .into_bytes()
}

fn decode(bytes: &[u8]) -> Scene3D {
    GltfDecoder::new().decode(bytes).unwrap()
}

fn encode_json(scene: &Scene3D) -> Value {
    let bytes = json_encoder().encode(scene).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn scene_with_sampler(sampler: Sampler) -> Scene3D {
    let mut scene = Scene3D::new();
    let mut tex = Texture::from_encoded("image/png".to_owned(), vec![0xFFu8; 8]);
    tex.sampler = sampler;
    scene.add_texture(tex);
    scene
}

// --- decode: undefined filters stay undefined -------------------------

#[test]
fn absent_filters_decode_as_undefined() {
    // A sampler declaring only wrap modes: both filters are undefined
    // per §3.8.4.1 and MUST NOT be coerced to an explicit choice.
    let scene = decode(&doc_with_sampler(r#"{ "wrapS": 33071 }"#));
    let s = scene.textures[0].sampler;
    assert_eq!(s.mag_filter, None, "absent magFilter stays undefined");
    assert_eq!(s.min_filter, None, "absent minFilter stays undefined");
    assert_eq!(s.wrap_s, WrapMode::ClampToEdge);
    assert_eq!(s.wrap_t, WrapMode::Repeat, "wrapT defaults to REPEAT");
}

#[test]
fn samplerless_texture_decodes_to_default_sampler() {
    let json = br#"{
        "asset": { "version": "2.0" },
        "images": [{ "uri": "data:image/png;base64,AAAA" }],
        "textures": [{ "source": 0 }]
    }"#;
    let scene = decode(json);
    assert_eq!(
        scene.textures[0].sampler,
        Sampler::default_sampler(),
        "no sampler object = repeat wrapping + undefined filters"
    );
}

#[test]
fn explicit_filters_decode_typed() {
    let scene = decode(&doc_with_sampler(
        r#"{ "magFilter": 9728, "minFilter": 9986, "wrapS": 33648, "wrapT": 33071 }"#,
    ));
    let s = scene.textures[0].sampler;
    assert_eq!(s.mag_filter, Some(MagFilter::Nearest));
    assert_eq!(s.min_filter, Some(MinFilter::NearestMipLinear));
    assert_eq!(s.wrap_s, WrapMode::MirroredRepeat);
    assert_eq!(s.wrap_t, WrapMode::ClampToEdge);
}

#[test]
fn explicit_linear_stays_distinguishable_from_undefined() {
    // Explicit LINEAR / trilinear are the values the old coercion
    // manufactured — an explicit declaration must now survive as
    // `Some(..)` while an omission stays `None`.
    let explicit = decode(&doc_with_sampler(
        r#"{ "magFilter": 9729, "minFilter": 9987 }"#,
    ));
    let s = explicit.textures[0].sampler;
    assert_eq!(s.mag_filter, Some(MagFilter::Linear));
    assert_eq!(s.min_filter, Some(MinFilter::LinearMipLinear));

    let undefined = decode(&doc_with_sampler(r#"{}"#));
    let u = undefined.textures[0].sampler;
    assert_eq!(u.mag_filter, None);
    assert_eq!(u.min_filter, None);
    assert_ne!(s, u, "explicit trilinear != undefined");
}

#[test]
fn all_six_min_filters_decode_typed() {
    let table = [
        (9728u32, MinFilter::Nearest),
        (9729, MinFilter::Linear),
        (9984, MinFilter::NearestMipNearest),
        (9985, MinFilter::LinearMipNearest),
        (9986, MinFilter::NearestMipLinear),
        (9987, MinFilter::LinearMipLinear),
    ];
    for (code, expected) in table {
        let scene = decode(&doc_with_sampler(&format!(r#"{{ "minFilter": {code} }}"#)));
        assert_eq!(
            scene.textures[0].sampler.min_filter,
            Some(expected),
            "minFilter {code}"
        );
    }
}

// --- encode: the wire shape mirrors the typed state -------------------

#[test]
fn default_sampler_emits_no_sampler_object() {
    // `Sampler::default_sampler()` is exactly the samplerless-texture
    // state, so the encoder emits neither a `samplers` array nor a
    // `texture.sampler` reference — the source shape round-trips.
    let json = encode_json(&scene_with_sampler(Sampler::default_sampler()));
    assert!(
        json.get("samplers").is_none(),
        "no samplers array for the default state, got {json}"
    );
    assert!(
        json["textures"][0].get("sampler").is_none(),
        "no sampler reference on the texture, got {json}"
    );
}

#[test]
fn undefined_filters_are_not_emitted() {
    // Non-default wrap forces a sampler object, but the undefined
    // filters must stay absent rather than gaining concrete values.
    let sampler = Sampler::default_sampler().with_wrap(WrapMode::ClampToEdge, WrapMode::Repeat);
    let json = encode_json(&scene_with_sampler(sampler));
    let s = &json["samplers"][0];
    assert!(s.get("magFilter").is_none(), "magFilter absent, got {s}");
    assert!(s.get("minFilter").is_none(), "minFilter absent, got {s}");
    assert_eq!(s["wrapS"], 33071);
    assert!(
        s.get("wrapT").is_none(),
        "REPEAT is the spec default and is omitted, got {s}"
    );
}

#[test]
fn explicit_filters_emit_exact_enums() {
    let sampler = Sampler::default_sampler()
        .with_mag_filter(MagFilter::Nearest)
        .with_min_filter(MinFilter::LinearMipNearest);
    let json = encode_json(&scene_with_sampler(sampler));
    let s = &json["samplers"][0];
    assert_eq!(s["magFilter"], 9728);
    assert_eq!(s["minFilter"], 9985);
}

#[test]
fn samplers_deduplicate_and_default_state_stays_out() {
    let shared = Sampler::default_sampler().with_mag_filter(MagFilter::Nearest);
    let mut scene = Scene3D::new();
    for _ in 0..2 {
        let mut tex = Texture::from_encoded("image/png".to_owned(), vec![0xFFu8; 8]);
        tex.sampler = shared;
        scene.add_texture(tex);
    }
    // Third texture carries the default state — no sampler for it.
    scene.add_texture(Texture::from_encoded(
        "image/png".to_owned(),
        vec![0xFFu8; 8],
    ));

    let json = encode_json(&scene);
    assert_eq!(
        json["samplers"].as_array().unwrap().len(),
        1,
        "two identical samplers deduplicate, the default state adds none"
    );
    assert_eq!(json["textures"][0]["sampler"], 0);
    assert_eq!(json["textures"][1]["sampler"], 0);
    assert!(json["textures"][2].get("sampler").is_none());
}

// --- full round-trips --------------------------------------------------

#[test]
fn undefined_filter_state_survives_encode_decode() {
    let original = Sampler::default_sampler().with_wrap(WrapMode::MirroredRepeat, WrapMode::Repeat);
    let scene = scene_with_sampler(original);
    let bytes = GltfEncoder::new().encode(&scene).unwrap();
    let decoded = GltfDecoder::new().decode(&bytes).unwrap();
    assert_eq!(decoded.textures[0].sampler, original);
    assert_eq!(decoded.textures[0].sampler.mag_filter, None);
}

#[test]
fn every_filter_combination_survives_encode_decode() {
    let mags = [None, Some(MagFilter::Nearest), Some(MagFilter::Linear)];
    let mins = [
        None,
        Some(MinFilter::Nearest),
        Some(MinFilter::Linear),
        Some(MinFilter::NearestMipNearest),
        Some(MinFilter::LinearMipNearest),
        Some(MinFilter::NearestMipLinear),
        Some(MinFilter::LinearMipLinear),
    ];
    for mag in mags {
        for min in mins {
            let original = Sampler {
                mag_filter: mag,
                min_filter: min,
                wrap_s: WrapMode::ClampToEdge,
                wrap_t: WrapMode::MirroredRepeat,
            };
            let scene = scene_with_sampler(original);
            let bytes = GltfEncoder::new().encode(&scene).unwrap();
            let decoded = GltfDecoder::new().decode(&bytes).unwrap();
            assert_eq!(
                decoded.textures[0].sampler, original,
                "sampler {original:?} must round-trip exactly"
            );
        }
    }
}
