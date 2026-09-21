//! Integration tests for the JMCC project system (`jmcc.toml`, dependencies, profiles).

use std::fs;
use std::path::Path;

use jmcc::compile_project;
use jmcc::project::Project;

fn create_test_project_env(base_dir: &Path) {
    drop(fs::remove_dir_all(base_dir));
    fs::create_dir_all(base_dir).unwrap();

    // 1. Dependency package "math_lib"
    let math_dir = base_dir.join("math_lib");
    fs::create_dir_all(math_dir.join("src")).unwrap();

    let math_manifest = r#"
        [package]
        name = "math_lib"
        version = "0.5.0"
        edition = 2026

        [profile.release]
        opt_level = 3
    "#;
    fs::write(math_dir.join("jmcc.toml"), math_manifest).unwrap();

    let math_src = r#"
        export function add(a: number, b: number) -> number {
            return a + b;
        }

        export function multiply(a: number, b: number) -> number {
            return a * b;
        }
    "#;
    fs::write(math_dir.join("src").join("lib.jc"), math_src).unwrap();

    // 2. Main application "app"
    let app_dir = base_dir.join("app");
    fs::create_dir_all(app_dir.join("src")).unwrap();

    let app_manifest = r#"
        [project]
        name = "my_app"
        version = "1.0.0"
        edition = 2026

        [dependencies]
        math = { path = "../math_lib" }

        [profile.dev]
        opt_level = 1

        [profile.release]
        opt_level = 3
    "#;
    fs::write(app_dir.join("jmcc.toml"), app_manifest).unwrap();

    let app_src = r#"
        import "math";

        event<player_join> {
            var sum = add(10, 20);
            var prod = multiply(sum, 2);
            player::message(prod);
        }
    "#;
    fs::write(app_dir.join("src").join("main.jc"), app_src).unwrap();
}

#[test]
fn test_compile_project_with_path_dependency() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_project_test_1");

    create_test_project_env(&test_dir);

    let app_dir = test_dir.join("app");
    let project = Project::open(&app_dir).expect("Open project");

    assert_eq!(project.name(), "my_app");
    assert_eq!(project.edition(), 2026);

    let entry = project.entry_point().expect("Locate entry point");
    assert_eq!(entry, app_dir.join("src").join("main.jc"));

    let graph = project
        .resolve_dependencies(false)
        .expect("Resolve dependencies");
    assert!(graph.get("math").is_some(), "math dependency resolved");

    // Compile using dev profile
    let module = compile_project(&project, Some("dev"), None)
        .expect("compile_project should succeed with path dependency");

    assert!(
        !module.handlers.is_empty(),
        "Generated module should contain handlers"
    );

    // Clean up
    drop(fs::remove_dir_all(&test_dir));
}

#[test]
fn test_project_profiles() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_project_test_2");

    create_test_project_env(&test_dir);

    let app_dir = test_dir.join("app");
    let project = Project::open(&app_dir).expect("Open project");

    let dev_profile = project.profile_options("dev");
    assert_eq!(dev_profile.opt_level, Some(1));

    let release_profile = project.profile_options("release");
    assert_eq!(release_profile.opt_level, Some(3));

    // Clean up
    drop(fs::remove_dir_all(&test_dir));
}

#[test]
fn test_project_find_upwards() {
    let test_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_project_test_3");

    create_test_project_env(&test_dir);

    let deep_nested_dir = test_dir.join("app").join("src");
    let found = Project::find(&deep_nested_dir)
        .expect("Project find")
        .expect("Project should be found from src/");

    assert_eq!(found.name(), "my_app");

    // Clean up
    drop(fs::remove_dir_all(&test_dir));
}

#[test]
fn test_create_package_bin_and_lib() {
    let base_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("tmp_new_pkg_test");
    drop(fs::remove_dir_all(&base_dir));

    // 1. Bin package
    let bin_dir = base_dir.join("test_bin_app");
    let bin_opts = jmcc::project::NewPackageOptions {
        name: Some("test_bin_app".into()),
        package_type: jmcc::project::PackageType::Binary,
        edition: 2026,
        vcs: None,
    };
    jmcc::project::create_package(&bin_dir, &bin_opts).expect("Create bin package");

    assert!(bin_dir.join("jmcc.toml").exists());
    assert!(bin_dir.join("src").join("main.jc").exists());
    assert!(bin_dir.join(".gitignore").exists());

    let manifest_content = fs::read_to_string(bin_dir.join("jmcc.toml")).unwrap();
    assert!(manifest_content.contains("name = \"test_bin_app\""));
    assert!(manifest_content.contains("edition = 2026"));

    // 2. Lib package
    let lib_dir = base_dir.join("test_lib_pkg");
    let lib_opts = jmcc::project::NewPackageOptions {
        name: Some("custom_lib".into()),
        package_type: jmcc::project::PackageType::Library,
        edition: 2026,
        vcs: None,
    };
    jmcc::project::create_package(&lib_dir, &lib_opts).expect("Create lib package");

    assert!(lib_dir.join("jmcc.toml").exists());
    assert!(lib_dir.join("src").join("lib.jc").exists());
    assert!(!lib_dir.join("src").join("main.jc").exists());

    let lib_manifest = fs::read_to_string(lib_dir.join("jmcc.toml")).unwrap();
    assert!(lib_manifest.contains("name = \"custom_lib\""));

    drop(fs::remove_dir_all(&base_dir));
}

#[test]
fn test_manifest_disable_action_limit_parsing() {
    let manifest_str = r#"
        [package]
        name = "custom_pkg"
        version = "1.0.0"
        edition = 2026
        disable_action_limit = true
    "#;
    let manifest: jmcc::project::Manifest = toml::from_str(manifest_str).expect("parse manifest");
    assert!(manifest.disable_action_limit());

    let manifest_default_str = r#"
        [package]
        name = "custom_pkg2"
        version = "1.0.0"
        edition = 2026
    "#;
    let manifest_default: jmcc::project::Manifest =
        toml::from_str(manifest_default_str).expect("parse default manifest");
    assert!(!manifest_default.disable_action_limit());
}

#[test]
fn test_manifest_upload_configuration() {
    // 1. In [package]
    let toml_pkg = r#"
        [package]
        name = "pkg_with_upload"
        version = "1.0.0"
        edition = 2026
        upload_target = "webhook"
        webhook_url = "https://discord.com/api/webhooks/123/abc"
    "#;
    let manifest: jmcc::project::Manifest = toml::from_str(toml_pkg).expect("parse manifest");
    assert_eq!(manifest.upload_target(), Some(jmcc::UploadTarget::Webhook));
    assert_eq!(
        manifest.webhook_url().as_deref(),
        Some("https://discord.com/api/webhooks/123/abc")
    );

    // 2. In [upload] section
    let toml_upload_sec = r#"
        [package]
        name = "pkg_with_upload_sec"
        version = "1.0.0"
        edition = 2026

        [upload]
        target = "webhook"
        webhook_url = "https://my-webhook.org"
        enabled = true
    "#;
    let manifest_sec: jmcc::project::Manifest =
        toml::from_str(toml_upload_sec).expect("parse upload section");
    assert_eq!(
        manifest_sec.upload_target(),
        Some(jmcc::UploadTarget::Webhook)
    );
    assert_eq!(
        manifest_sec.webhook_url().as_deref(),
        Some("https://my-webhook.org")
    );
    assert!(manifest_sec.auto_upload());

    // 3. [upload] overrides [package]
    let toml_override = r#"
        [package]
        name = "pkg_override"
        version = "1.0.0"
        edition = 2026
        upload_target = "official"
        webhook_url = "https://package-webhook.org"

        [upload]
        target = "webhook"
        webhook_url = "https://upload-webhook.org"
    "#;
    let manifest_over: jmcc::project::Manifest =
        toml::from_str(toml_override).expect("parse override");
    assert_eq!(
        manifest_over.upload_target(),
        Some(jmcc::UploadTarget::Webhook)
    );
    assert_eq!(
        manifest_over.webhook_url().as_deref(),
        Some("https://upload-webhook.org")
    );

    // 4. Default when unspecified
    let toml_default = r#"
        [package]
        name = "pkg_default"
        version = "1.0.0"
        edition = 2026
    "#;
    let manifest_def: jmcc::project::Manifest =
        toml::from_str(toml_default).expect("parse default");
    assert_eq!(manifest_def.upload_target(), None);
    assert_eq!(manifest_def.webhook_url(), None);
    assert!(!manifest_def.auto_upload());
}

#[test]
fn test_profile_upload_configuration() {
    let toml_profile = r#"
        [package]
        name = "pkg_profile"
        version = "1.0.0"
        edition = 2026

        [profile.release]
        upload_target = "webhook"
        webhook_url = "https://discord.com/profile-webhook"
    "#;
    let manifest: jmcc::project::Manifest = toml::from_str(toml_profile).expect("parse profile");
    let rel_profile = manifest
        .profile
        .get("release")
        .expect("release profile exists");
    assert_eq!(rel_profile.upload_target, Some(jmcc::UploadTarget::Webhook));
    assert_eq!(
        rel_profile.webhook_url.as_deref(),
        Some("https://discord.com/profile-webhook")
    );

    let mut options = jmcc::CompileOptions::default();
    rel_profile.apply_to(&mut options);
    assert_eq!(options.upload_target, Some(jmcc::UploadTarget::Webhook));
    assert_eq!(
        options.webhook_url.as_deref(),
        Some("https://discord.com/profile-webhook")
    );
}
