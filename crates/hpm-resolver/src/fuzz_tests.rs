//! Fuzz tests for HPM resolver parsing
//!
//! These tests use fuzzcheck for structure-aware fuzzing.
//! Run with: cargo +nightly test fuzz_ --release -p hpm-resolver --ignored

#[cfg(test)]
mod fuzz {
    use fuzzcheck::fuzz_test;

    /// Fuzz test for version requirement parsing
    ///
    /// This tests that parsing arbitrary strings as version requirements
    /// doesn't cause panics or undefined behavior.
    #[test]
    #[ignore] // Run explicitly with: cargo +nightly test fuzz_version_req_parsing --release --ignored
    fn fuzz_version_req_parsing() {
        use crate::version::VersionConstraint;
        use std::str::FromStr;

        let result = fuzz_test(|input: &String| {
            // Parse should never panic, only return errors
            let _ = VersionConstraint::from_str(input);
        })
        .default_mutator()
        .serde_serializer()
        .default_sensor_and_pool()
        .arguments_from_cargo_fuzzcheck()
        .stop_after_first_test_failure(true)
        .launch();

        assert!(!result.found_test_failure, "Fuzzing found a failure");
    }

    /// Fuzz test for version string parsing
    #[test]
    #[ignore]
    fn fuzz_version_parsing() {
        use crate::version::Version;
        use std::str::FromStr;

        let result = fuzz_test(|input: &String| {
            // Parse should never panic, only return errors
            let _ = Version::from_str(input);
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
