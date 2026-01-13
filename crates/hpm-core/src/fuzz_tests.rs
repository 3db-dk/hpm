//! Fuzz tests for HPM core parsing
//!
//! These tests use fuzzcheck for structure-aware fuzzing.
//! Run with: cargo +nightly test fuzz_ --release -p hpm-core --ignored

#[cfg(test)]
mod fuzz {
    use fuzzcheck::fuzz_test;

    /// Fuzz test for lock file TOML parsing
    ///
    /// This tests that parsing arbitrary strings as lock files
    /// doesn't cause panics or undefined behavior.
    #[test]
    #[ignore] // Run explicitly with: cargo +nightly test fuzz_lock_file_parsing --release --ignored
    fn fuzz_lock_file_parsing() {
        use crate::LockFile;

        let result = fuzz_test(|input: &String| {
            // Parse should never panic, only return errors
            let _ = toml::from_str::<LockFile>(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }

    /// Fuzz test for package spec parsing
    #[test]
    #[ignore]
    fn fuzz_package_spec_parsing() {
        use crate::storage::types::PackageSpec;
        use std::str::FromStr;

        let result = fuzz_test(|input: &String| {
            // Parse should never panic, only return errors
            let _ = PackageSpec::from_str(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }
}
