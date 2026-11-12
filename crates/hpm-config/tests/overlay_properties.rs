//! Properties of `ConfigOverlay`, the shape every config file is parsed as:
//! it round-trips through TOML exactly, and serializing writes only the
//! fields that are set (an unset field must never become an explicit
//! default in a written file).

use std::path::{Path, PathBuf};

use hpm_config::overlay::{
    ConfigOverlay, InstallOverlay, ProjectsOverlay, SigningOverlay, StorageOverlay,
};
use hpm_config::registry::{RegistrySourceConfig, RegistryType};
use proptest::prelude::*;

fn path_strategy() -> impl Strategy<Value = PathBuf> {
    prop_oneof![
        "[a-z/._-]{1,20}",
        Just("/home/ø user/.hpm".to_string()),
        Just("C:\\Users\\me\\.hpm".to_string()),
        Just("~/日本/packages".to_string()),
    ]
    .prop_map(PathBuf::from)
}

fn opt<S: Strategy>(s: S) -> impl Strategy<Value = Option<S::Value>> {
    prop::option::of(s)
}

fn registry_strategy() -> impl Strategy<Value = RegistrySourceConfig> {
    (
        prop::sample::select(vec!["tumbletrove", "studio", "mirror"]),
        prop::sample::select(vec![
            "https://api.tumbletrove.com/v1/registry",
            "git@example.com:studio/registry.git",
        ]),
        prop::sample::select(vec![RegistryType::Api, RegistryType::Git]),
    )
        .prop_map(|(name, url, registry_type)| RegistrySourceConfig {
            name: name.to_string(),
            url: url.to_string(),
            registry_type,
        })
}

fn overlay_strategy() -> impl Strategy<Value = ConfigOverlay> {
    // `path` is a legacy key that is read but never written, so a written
    // overlay never holds it.
    let install = opt((1usize..64).prop_map(|n| std::num::NonZeroUsize::new(n).unwrap())).prop_map(
        |parallel_downloads| InstallOverlay {
            path: None,
            parallel_downloads,
        },
    );
    let storage = (
        opt(path_strategy()),
        opt(path_strategy()),
        opt(path_strategy()),
        opt(path_strategy()),
    )
        .prop_map(
            |(home_dir, cache_dir, packages_dir, registry_cache_dir)| StorageOverlay {
                home_dir,
                cache_dir,
                packages_dir,
                registry_cache_dir,
            },
        );
    let projects = (
        opt(prop::collection::vec(path_strategy(), 0..3)),
        opt(prop::collection::vec(path_strategy(), 0..3)),
        opt(0usize..10),
        opt(prop::collection::vec("[a-z*/.]{1,10}", 0..3)),
    )
        .prop_map(
            |(explicit_paths, search_roots, max_search_depth, ignore_patterns)| ProjectsOverlay {
                explicit_paths,
                search_roots,
                max_search_depth,
                ignore_patterns,
            },
        );
    let signing = opt(path_strategy()).prop_map(|key_path| SigningOverlay { key_path });
    (
        opt(install),
        opt(storage),
        opt(projects),
        opt(prop::collection::vec(registry_strategy(), 0..3)),
        opt(signing),
    )
        .prop_map(
            |(install, storage, projects, registries, signing)| ConfigOverlay {
                install,
                storage,
                projects,
                registries,
                signing,
            },
        )
}

/// Count the `Some` leaves of an overlay: the fields a file sets.
fn set_fields(o: &ConfigOverlay) -> usize {
    let mut n = 0;
    if let Some(i) = &o.install {
        n += 1 + i.parallel_downloads.is_some() as usize;
    }
    if let Some(s) = &o.storage {
        n += 1
            + s.home_dir.is_some() as usize
            + s.cache_dir.is_some() as usize
            + s.packages_dir.is_some() as usize
            + s.registry_cache_dir.is_some() as usize;
    }
    if let Some(p) = &o.projects {
        n += 1
            + p.explicit_paths.is_some() as usize
            + p.search_roots.is_some() as usize
            + p.max_search_depth.is_some() as usize
            + p.ignore_patterns.is_some() as usize;
    }
    if o.registries.is_some() {
        n += 1;
    }
    if let Some(s) = &o.signing {
        n += 1 + s.key_path.is_some() as usize;
    }
    n
}

/// Count the keys a TOML document writes, descending into tables (an array
/// of tables counts as one key, like `registries`).
fn written_keys(v: &toml::Value) -> usize {
    match v {
        toml::Value::Table(t) => t
            .values()
            .map(|child| match child {
                toml::Value::Table(_) => 1 + written_keys(child),
                _ => 1,
            })
            .sum(),
        _ => 0,
    }
}

proptest! {
    #[test]
    fn overlay_round_trips_through_toml(overlay in overlay_strategy()) {
        let text = toml::to_string(&overlay).unwrap();
        let back = ConfigOverlay::parse(&text, Path::new("config.toml")).unwrap();
        prop_assert_eq!(back, overlay);
    }

    #[test]
    fn serializing_writes_only_the_fields_that_are_set(overlay in overlay_strategy()) {
        let value = toml::Value::try_from(&overlay).unwrap();
        prop_assert_eq!(written_keys(&value), set_fields(&overlay));
    }
}

/// A config file as `Config::save` writes it (TumbleTrove Desktop has saved
/// users' `~/.hpm/config.toml` this way: every field, resolved), captured
/// from hpm 0.37. It must keep loading, and loading it must reproduce the
/// values it holds.
#[test]
fn a_config_written_by_config_save_still_loads() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config_saved_by_0_37.toml");
    let config = hpm_config::Config::load_from_path(&path).unwrap();
    assert_eq!(config.storage.home_dir, PathBuf::from("/Users/artist/.hpm"));
    assert_eq!(
        config.storage.cache_dir,
        PathBuf::from("/Users/artist/.hpm/cache")
    );
    assert_eq!(config.registries.len(), 1);
    assert_eq!(config.registries[0].name, "tumbletrove");
    assert_eq!(config.projects.max_search_depth, 3);
}

/// A key hpm never wrote, in any section, fails the load and is named; the
/// legacy `[install] path` (written by older hpm, never read) is accepted.
#[test]
fn unknown_config_keys_are_refused_and_the_legacy_install_path_is_accepted() {
    for (doc, key) in [
        ("[registry]\nname = \"x\"\n", "registry"),
        ("[install]\nparalel_downloads = 4\n", "paralel_downloads"),
        ("[storage]\nhome = \"/x\"\n", "home"),
        ("[projects]\nsearch_root = []\n", "search_root"),
        ("[signing]\nkey = \"/k\"\n", "key"),
        (
            "[[registries]]\nname = \"x\"\nurl = \"u\"\ntype = \"api\"\ntoken = \"t\"\n",
            "token",
        ),
    ] {
        let err = ConfigOverlay::parse(doc, Path::new("config.toml")).unwrap_err();
        let source = std::error::Error::source(&err).unwrap().to_string();
        assert!(source.contains(key), "{doc}: {source}");
    }
    let legacy =
        ConfigOverlay::parse("[install]\npath = \"packages/hpm\"\n", Path::new("c.toml")).unwrap();
    assert_eq!(
        legacy.install.unwrap().path.as_deref(),
        Some("packages/hpm")
    );
    assert!(
        ConfigOverlay::parse("[install]\nparallel_downloads = 0\n", Path::new("c.toml")).is_err()
    );
}
