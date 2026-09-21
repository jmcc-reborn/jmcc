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

#[test]
fn game_values_and_actions_accessors_work() {
    use crate::generated::{
        canonicalize_object, get_action_defs, get_actions_for_object, get_event_def,
        get_event_defs, get_game_value, get_game_values, get_known_objects,
    };

    let gvs = get_game_values();
    assert!(!gvs.is_empty(), "game values list must not be empty");
    let health = get_game_value("absorption_health").expect("absorption_health should exist");
    assert_eq!(health.value_type, "number");

    let evs = get_event_defs();
    assert!(!evs.is_empty(), "event defs must not be empty");
    let join_ev = get_event_def("player_join").expect("player_join should exist");
    assert_eq!(join_ev.id, "player_join");

    let action_defs = get_action_defs();
    assert!(!action_defs.is_empty(), "actions must not be empty");

    let world_actions: Vec<_> = get_actions_for_object("world").collect();
    assert!(!world_actions.is_empty(), "world actions must not be empty");

    let mir_actions_count = get_actions_for_object("мир").count();
    assert_eq!(world_actions.len(), mir_actions_count);

    assert_eq!(canonicalize_object("world"), Some("world"));
    assert_eq!(canonicalize_object("мир"), Some("world"));
    assert_eq!(canonicalize_object("value"), Some("value"));
    assert_eq!(canonicalize_object("значение"), Some("value"));
    assert_eq!(canonicalize_object("player"), Some("player"));
    assert_eq!(canonicalize_object("игрок"), Some("player"));

    let objects = get_known_objects();
    assert!(
        objects
            .iter()
            .any(|(en, ru)| *en == "world" && *ru == "мир")
    );
}
