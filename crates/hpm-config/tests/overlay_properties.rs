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
    let install = (opt("[a-z./]{1,12}"), opt(0usize..64)).prop_map(|(path, parallel_downloads)| {
        InstallOverlay {
            path,
            parallel_downloads,
        }
    });
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
        n += 1 + i.path.is_some() as usize + i.parallel_downloads.is_some() as usize;
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
