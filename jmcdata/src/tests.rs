use std::fs;

use crate::module::Module;

#[test]
fn deserializes_and_roundtrips_a_reference_module() {
    let source = fs::read_to_string("assets/tests/test1.json").expect("fixture is readable");
    let module: Module<'_> = serde_json::from_str(&source).expect("fixture deserializes");

    assert_eq!(module.handlers.len(), 1);
    serde_json::to_string(&module).expect("module serializes back");
}

/// `heck` pascal-cases `XZ` and `XYZ` to identifiers that collide with those of
/// other spellings, so the variant name cannot be derived from the schema string
/// at serialization time. `as_str` exists to keep both spellings intact.
#[test]
fn enum_identifiers_roundtrip_through_serde() {
    use crate::generated::ClampLocationCoordinatesMode as Mode;

    for mode in [Mode::Xyz, Mode::Xz, Mode::Y] {
        assert_eq!(
            serde_json::to_string(&mode).expect("variant serializes"),
            format!("\"{}\"", mode.as_str())
        );
    }

    assert_eq!(Mode::Xyz.as_str(), "XYZ");
    assert_eq!(Mode::Xz.as_str(), "XZ");
}
