use super::*;
use base64::Engine;
use std::fs;
use tempfile::TempDir;

fn create_test_package(dir: &Path) {
    fs::write(
        dir.join("hpm.toml"),
        r#"[package]
path = "studio/test-pkg"
name = "Test Package"
version = "1.0.0"
"#,
    )
    .unwrap();
    fs::write(dir.join("README.md"), "# Test").unwrap();
    fs::create_dir_all(dir.join("otls")).unwrap();
    fs::write(dir.join("otls/tool.hda"), b"hda-content").unwrap();
}

#[test]
fn ignore_rules_exclude_git_and_hpm() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::create_dir_all(dir.path().join(".hpm")).unwrap();

    let rules = build_ignore_rules(dir.path()).unwrap();

    assert!(
        rules
            .matched_path_or_any_parents(Path::new(".git/config"), false)
            .is_ignore()
    );
    assert!(
        rules
            .matched_path_or_any_parents(Path::new(".hpm/config.toml"), false)
            .is_ignore()
    );
    assert!(
        !rules
            .matched_path_or_any_parents(Path::new("hpm.toml"), false)
            .is_ignore()
    );
}

#[test]
fn ignore_rules_load_gitignore_and_hpmignore() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(dir.path().join(".hpmignore"), "build/\n").unwrap();

    let rules = build_ignore_rules(dir.path()).unwrap();

    assert!(
        rules
            .matched_path_or_any_parents(Path::new("debug.log"), false)
            .is_ignore()
    );
    assert!(
        rules
            .matched_path_or_any_parents(Path::new("build/out.o"), false)
            .is_ignore()
    );
    assert!(
        !rules
            .matched_path_or_any_parents(Path::new("src/main.rs"), false)
            .is_ignore()
    );
}

#[test]
fn archive_contains_expected_files() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());
    // Add dirs that should be excluded
    fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
    fs::write(dir.path().join(".git/config"), "gitconfig").unwrap();
    fs::create_dir_all(dir.path().join(".hpm")).unwrap();
    fs::write(dir.path().join(".hpm/config.toml"), "").unwrap();

    let output_dir = TempDir::new().unwrap();
    let ignore = build_ignore_rules(dir.path()).unwrap();
    let archive_path = create_archive(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        &ignore,
        None,
        None,
        ArchiveLayout::default(),
    )
    .unwrap();

    assert!(archive_path.exists());
    assert_eq!(archive_path.file_name().unwrap(), "test-pkg-1.0.0.zip");

    // Verify contents
    let file = fs::File::open(&archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    assert!(names.contains(&"hpm.toml".to_string()));
    assert!(names.contains(&"README.md".to_string()));
    assert!(names.contains(&"otls/tool.hda".to_string()));
    assert!(!names.iter().any(|n| n.starts_with(".git")));
    assert!(!names.iter().any(|n| n.starts_with(".hpm")));
}

#[test]
fn checksum_is_deterministic() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let out1 = TempDir::new().unwrap();
    let out2 = TempDir::new().unwrap();
    let ignore = build_ignore_rules(dir.path()).unwrap();

    let path1 = create_archive(
        dir.path(),
        "test-pkg",
        "1.0.0",
        out1.path(),
        &ignore,
        None,
        None,
        ArchiveLayout::default(),
    )
    .unwrap();
    let path2 = create_archive(
        dir.path(),
        "test-pkg",
        "1.0.0",
        out2.path(),
        &ignore,
        None,
        None,
        ArchiveLayout::default(),
    )
    .unwrap();

    let cksum1 = compute_archive_checksum(&path1).unwrap();
    let cksum2 = compute_archive_checksum(&path2).unwrap();

    assert_eq!(cksum1, cksum2);
    assert_eq!(cksum1.len(), 64); // SHA-256 hex is 64 chars
}

#[test]
fn sign_and_verify_roundtrip() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let ignore = build_ignore_rules(dir.path()).unwrap();
    let archive_path = create_archive(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        &ignore,
        None,
        None,
        ArchiveLayout::default(),
    )
    .unwrap();

    // Generate a keypair for testing
    let secret = [42u8; 32];
    let signing_key = SigningKey::from_bytes(&secret);

    let (sig_b64, key_id) = sign_archive(&archive_path, &signing_key).unwrap();

    // Verify signature
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sig_b64)
        .unwrap();
    let signature = ed25519_dalek::Signature::from_bytes(sig_bytes.as_slice().try_into().unwrap());

    let archive_bytes = fs::read(&archive_path).unwrap();
    let verifying_key = signing_key.verifying_key();
    assert!(ed25519_dalek::Verifier::verify(&verifying_key, &archive_bytes, &signature).is_ok());

    // key_id is first 8 bytes of public key in hex = 16 chars
    assert_eq!(key_id.len(), 16);
}

#[test]
fn invalid_key_file_not_pem() {
    let dir = TempDir::new().unwrap();
    let key_path = dir.path().join("bad.pem");
    fs::write(&key_path, b"garbage").unwrap();

    let result = load_signing_key(&key_path);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), PackError::SigningKey(_)));
}

#[test]
fn invalid_key_file_not_found() {
    let result = load_signing_key(Path::new("/nonexistent/key.bin"));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), PackError::SigningKey(_)));
}

#[test]
fn load_signing_key_pem_roundtrip() {
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;

    let original = SigningKey::from_bytes(&[7u8; 32]);
    let pem = original.to_pkcs8_pem(LineEnding::LF).unwrap();

    let dir = TempDir::new().unwrap();
    let key_path = dir.path().join("signing.pem");
    fs::write(&key_path, pem.as_bytes()).unwrap();

    let loaded = load_signing_key(&key_path).unwrap();
    assert_eq!(
        loaded.verifying_key().to_bytes(),
        original.verifying_key().to_bytes()
    );
}

#[test]
fn load_signing_key_from_pem_inline() {
    use ed25519_dalek::pkcs8::EncodePrivateKey;
    use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;

    let original = SigningKey::from_bytes(&[9u8; 32]);
    let pem = original.to_pkcs8_pem(LineEnding::LF).unwrap();

    let loaded = load_signing_key_from_pem(&pem).unwrap();
    assert_eq!(
        loaded.verifying_key().to_bytes(),
        original.verifying_key().to_bytes()
    );
}

#[test]
fn pack_without_signing() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        None,
        &StageConfig::default(),
        ArchiveLayout::default(),
    )
    .unwrap();

    assert!(result.archive_path.exists());
    assert!(!result.checksum.is_empty());
    assert!(result.signature.is_none());
    assert!(result.key_id.is_none());
}

#[test]
fn pack_with_signing() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let secret = [7u8; 32];
    let signing_key = SigningKey::from_bytes(&secret);

    let output_dir = TempDir::new().unwrap();
    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        Some(&signing_key),
        None,
        &StageConfig::default(),
        ArchiveLayout::default(),
    )
    .unwrap();

    assert!(result.archive_path.exists());
    assert!(!result.checksum.is_empty());
    assert!(result.signature.is_some());
    assert!(result.key_id.is_some());
}

fn create_native_test_package(dir: &Path) {
    create_test_package(dir);
    fs::create_dir_all(dir.join("lib/linux-x86_64")).unwrap();
    fs::write(dir.join("lib/linux-x86_64/libfoo.so"), b"elf-binary").unwrap();
    fs::create_dir_all(dir.join("lib/macos-aarch64")).unwrap();
    fs::write(dir.join("lib/macos-aarch64/libfoo.dylib"), b"macho-binary").unwrap();
    fs::create_dir_all(dir.join("lib/windows-x86_64")).unwrap();
    fs::write(dir.join("lib/windows-x86_64/foo.dll"), b"pe-binary").unwrap();
}

fn test_stage_config() -> hpm_package::manifest::StageConfig {
    use hpm_package::manifest::{PlaceRule, PlatformStaging, StageConfig, StagePlatformRules};
    let mut entries = indexmap::IndexMap::new();
    for plat in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
        entries.insert(
            plat.to_string(),
            StagePlatformRules {
                place: vec![PlaceRule {
                    from: format!("lib/{}/*", plat),
                    to: format!("lib/{}/", plat),
                }],
            },
        );
    }
    StageConfig {
        platform: PlatformStaging { entries },
        ..Default::default()
    }
}

#[test]
fn platform_archive_name_includes_platform() {
    let dir = TempDir::new().unwrap();
    create_native_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let stage_config = test_stage_config();
    let platform = hpm_package::platform::Platform::LinuxX86_64;

    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        Some(&platform),
        &stage_config,
        ArchiveLayout::default(),
    )
    .unwrap();

    assert_eq!(
        result.archive_path.file_name().unwrap(),
        "test-pkg-1.0.0-linux-x86_64.zip"
    );
    assert_eq!(result.platform.as_deref(), Some("linux-x86_64"));
}

#[test]
fn platform_archive_excludes_other_platforms() {
    let dir = TempDir::new().unwrap();
    create_native_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let stage_config = test_stage_config();
    let platform = hpm_package::platform::Platform::LinuxX86_64;

    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        Some(&platform),
        &stage_config,
        ArchiveLayout::default(),
    )
    .unwrap();

    let file = fs::File::open(&result.archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    // Should contain linux files
    assert!(names.contains(&"lib/linux-x86_64/libfoo.so".to_string()));
    // Should NOT contain other platforms
    assert!(!names.iter().any(|n| n.contains("macos-aarch64")));
    assert!(!names.iter().any(|n| n.contains("windows-x86_64")));
    // Should still contain shared files
    assert!(names.contains(&"hpm.toml".to_string()));
    assert!(names.contains(&"README.md".to_string()));
}

#[test]
fn shared_glob_across_platforms_rides_through_each_archive() {
    // A glob listed identically under every platform declares common
    // content with a shared install path; the matched files must
    // appear in every per-platform archive.
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());
    fs::create_dir_all(dir.path().join("resolver/houdini21")).unwrap();
    fs::write(
        dir.path().join("resolver/houdini21/foo.dll"),
        b"shared-binary",
    )
    .unwrap();

    // The same `from` glob listed under every platform declares
    // common content with a shared install path — the place rule's
    // `to = "resolver/"` plus the basename-only rewrite keeps the
    // file at its original layout in every per-platform archive.
    let mut entries = indexmap::IndexMap::new();
    for plat in ["linux-x86_64", "macos-aarch64", "windows-x86_64"] {
        entries.insert(
            plat.to_string(),
            hpm_package::manifest::StagePlatformRules {
                place: vec![hpm_package::manifest::PlaceRule {
                    from: "resolver/houdini*/**/*".to_string(),
                    to: "resolver/houdini21/".to_string(),
                }],
            },
        );
    }
    let stage_config = hpm_package::manifest::StageConfig {
        platform: hpm_package::manifest::PlatformStaging { entries },
        ..Default::default()
    };

    for platform in [
        hpm_package::platform::Platform::LinuxX86_64,
        hpm_package::platform::Platform::MacosAarch64,
        hpm_package::platform::Platform::WindowsX86_64,
    ] {
        let output_dir = TempDir::new().unwrap();
        let result = pack(
            dir.path(),
            "test-pkg",
            "1.0.0",
            output_dir.path(),
            None,
            Some(&platform),
            &stage_config,
            ArchiveLayout::default(),
        )
        .unwrap();

        let file = fs::File::open(&result.archive_path).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(
            names.contains(&"resolver/houdini21/foo.dll".to_string()),
            "shared resolver binary missing from {} archive: {:?}",
            platform,
            names
        );
    }
}

#[test]
fn target_glob_overrides_other_platform_match() {
    // A path claimed by the target wins over an exclude from another
    // platform's glob.
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());
    fs::create_dir_all(dir.path().join("shared")).unwrap();
    fs::write(dir.path().join("shared/binary.so"), b"data").unwrap();

    // Linux claims `shared/*`; macOS claims `shared/*` AND
    // `lib/macos-aarch64/*`. When packing for Linux, `shared/binary.so`
    // matches the target's rule (kept), and we also confirm the file
    // isn't dropped just because macOS also lists `shared/*`.
    let mut entries = indexmap::IndexMap::new();
    entries.insert(
        "linux-x86_64".to_string(),
        hpm_package::manifest::StagePlatformRules {
            place: vec![hpm_package::manifest::PlaceRule {
                from: "shared/*".to_string(),
                to: "shared/".to_string(),
            }],
        },
    );
    entries.insert(
        "macos-aarch64".to_string(),
        hpm_package::manifest::StagePlatformRules {
            place: vec![
                hpm_package::manifest::PlaceRule {
                    from: "shared/*".to_string(),
                    to: "shared/".to_string(),
                },
                hpm_package::manifest::PlaceRule {
                    from: "lib/macos-aarch64/*".to_string(),
                    to: "lib/macos-aarch64/".to_string(),
                },
            ],
        },
    );
    let stage_config = hpm_package::manifest::StageConfig {
        platform: hpm_package::manifest::PlatformStaging { entries },
        ..Default::default()
    };

    let output_dir = TempDir::new().unwrap();
    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        Some(&hpm_package::platform::Platform::LinuxX86_64),
        &stage_config,
        ArchiveLayout::default(),
    )
    .unwrap();

    let file = fs::File::open(&result.archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    assert!(names.contains(&"shared/binary.so".to_string()));
}

#[test]
fn pack_without_platform_has_no_platform_tag() {
    let dir = TempDir::new().unwrap();
    create_native_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        None,
        &StageConfig::default(),
        ArchiveLayout::default(),
    )
    .unwrap();

    assert_eq!(
        result.archive_path.file_name().unwrap(),
        "test-pkg-1.0.0.zip"
    );
    assert!(result.platform.is_none());
}

#[test]
fn inject_files_added_to_archive() {
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let inject = vec![(
        "test-pkg.json".to_string(),
        b"{\"name\": \"test-pkg\"}".to_vec(),
    )];

    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        None,
        &StageConfig::default(),
        ArchiveLayout {
            inject_files: &inject,
            content_prefix: None,
        },
    )
    .unwrap();

    let file = fs::File::open(&result.archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    assert!(names.contains(&"test-pkg.json".to_string()));
    assert!(names.contains(&"hpm.toml".to_string()));

    // Verify injected file content
    let mut injected = zip.by_name("test-pkg.json").unwrap();
    let mut content = String::new();
    std::io::Read::read_to_string(&mut injected, &mut content).unwrap();
    assert_eq!(content, "{\"name\": \"test-pkg\"}");
}

#[test]
fn content_prefix_produces_hpackage_layout() {
    // With a content prefix (the package slug), staged content lands under
    // `{slug}/` while injected files (the generated `{slug}.json`) stay at
    // the archive root — the layout Houdini's package system expects when
    // the archive is extracted straight into a packages directory.
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());

    let output_dir = TempDir::new().unwrap();
    let inject = vec![(
        "test-pkg.json".to_string(),
        b"{\"hpath\": \"$HOUDINI_PACKAGE_PATH/test-pkg\"}".to_vec(),
    )];

    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        None,
        &StageConfig::default(),
        ArchiveLayout {
            inject_files: &inject,
            content_prefix: Some("test-pkg"),
        },
    )
    .unwrap();

    let file = fs::File::open(&result.archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    // Injected json at the root; all content under the slug folder.
    assert!(names.contains(&"test-pkg.json".to_string()));
    assert!(names.contains(&"test-pkg/hpm.toml".to_string()));
    assert!(names.contains(&"test-pkg/otls/tool.hda".to_string()));
    // Nothing else at the root.
    for name in &names {
        assert!(
            name == "test-pkg.json" || name.starts_with("test-pkg/"),
            "unexpected root-level entry: {name}"
        );
    }
}

#[test]
fn hand_written_json_ships_once_at_root_under_prefix() {
    // A hand-written {slug}.json in the package dir is injected (by content)
    // at the archive root; the staged copy is skipped so it isn't shipped
    // twice (once at the root, once under the content prefix).
    let dir = TempDir::new().unwrap();
    create_test_package(dir.path());
    fs::write(dir.path().join("test-pkg.json"), b"{\"hand\": true}").unwrap();

    let output_dir = TempDir::new().unwrap();
    let inject = vec![("test-pkg.json".to_string(), b"{\"hand\": true}".to_vec())];

    let result = pack(
        dir.path(),
        "test-pkg",
        "1.0.0",
        output_dir.path(),
        None,
        None,
        &StageConfig::default(),
        ArchiveLayout {
            inject_files: &inject,
            content_prefix: Some("test-pkg"),
        },
    )
    .unwrap();

    let file = fs::File::open(&result.archive_path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();

    assert_eq!(
        names.iter().filter(|n| n.contains("test-pkg.json")).count(),
        1,
        "json shipped more than once: {names:?}"
    );
    assert!(names.contains(&"test-pkg.json".to_string()));

    let mut injected = zip.by_name("test-pkg.json").unwrap();
    let mut content = String::new();
    std::io::Read::read_to_string(&mut injected, &mut content).unwrap();
    assert_eq!(content, "{\"hand\": true}");
}
