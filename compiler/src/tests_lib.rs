use super::*;
#[test]
fn project_names_enforce_length_and_normalize_surrounding_whitespace() {
    let root = env!("CARGO_MANIFEST_DIR");
    assert!(project_path(&"a".repeat(81), root).is_err());
    assert!(project_path(&"a".repeat(80), root).is_ok());
    assert_eq!(
        project_path("  Demo QA  ", root).unwrap(),
        Path::new(root).join("Demo QA")
    );
    assert!(project_path("Demo", "").is_err());
}
#[test]
fn installation_copies_models_and_nested_dependencies() {
    let destination =
        std::env::temp_dir().join(format!("engine-assets-{}", std::process::id()));
    fs::create_dir(&destination).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("apps/game/assets");
    let result = copy_assets(&source, &destination);
    let model = fs::read(destination.join("props/robot.glb"));
    let texture = fs::read(destination.join("textures/crate.png"));
    let scene = fs::read(destination.join("scenes/robots.json"));
    fs::remove_dir_all(&destination).unwrap();
    result.unwrap();
    assert_eq!(
        model.unwrap(),
        fs::read(source.join("props/robot.glb")).unwrap()
    );
    assert_eq!(
        texture.unwrap(),
        fs::read(source.join("textures/crate.png")).unwrap()
    );
    assert_eq!(
        scene.unwrap(),
        fs::read(source.join("scenes/robots.json")).unwrap()
    );
}
#[test]
fn rejects_unsafe_names_and_destinations() {
    for name in ["", "../escape", "a/b", "CON", "Lpt1", "a:stream"] {
        assert!(project_path(name, env!("CARGO_MANIFEST_DIR")).is_err());
    }
    assert!(project_path("Demo", "relative").is_err());
}
#[test]
fn resolves_new_project_and_rejects_existing_folder() {
    let root = env!("CARGO_MANIFEST_DIR");
    assert_eq!(
        project_path("Triangle Demo", root).unwrap(),
        Path::new(root).join("Triangle Demo")
    );
    assert!(project_path("src", root).is_err());
}
