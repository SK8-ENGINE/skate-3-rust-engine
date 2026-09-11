use skate_mods::validate_package;
use std::path::PathBuf;

#[test]
fn physics_sandbox_validates() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../sdk/examples/physics-sandbox");
    let m = validate_package(&root).expect("package should validate");
    assert_eq!(m.api, 2);
    assert_eq!(m.id, "examples.physics-sandbox");
}
