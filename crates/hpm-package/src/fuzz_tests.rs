//! Fuzz tests for HPM package parsing
//!
//! These tests use fuzzcheck for structure-aware fuzzing.
//! Run with: cargo +nightly test fuzz_ --release -p hpm-package

#[cfg(test)]
mod fuzz {
    use fuzzcheck::fuzz_test;

    /// Fuzz test for TOML manifest parsing
    ///
    /// This tests that parsing arbitrary strings as TOML manifests
    /// doesn't cause panics or undefined behavior.
    #[test]
    #[ignore] // Run explicitly with: cargo +nightly test fuzz_manifest_parsing --release --ignored
    fn fuzz_manifest_parsing() {
        use crate::PackageManifest;

        let result = fuzz_test(|input: &String| {
            // Parse should never panic, only return errors
            let _ = toml::from_str::<PackageManifest>(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }

    /// Fuzz test for dependency spec JSON parsing
    #[test]
    #[ignore]
    fn fuzz_dependency_spec_json() {
        use crate::DependencySpec;

        let result = fuzz_test(|input: &String| {
            // JSON parsing should never panic
            let _ = serde_json::from_str::<DependencySpec>(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }

    /// Fuzz test for Python dependency spec parsing
    #[test]
    #[ignore]
    fn fuzz_python_dependency_spec() {
        use crate::PythonDependencySpec;

        let result = fuzz_test(|input: &String| {
            // JSON parsing should never panic
            let _ = serde_json::from_str::<PythonDependencySpec>(input);
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
