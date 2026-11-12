//! `HoudiniRange::matches_version` against an independent model of
//! Houdini's own evaluation of the emitted `enable` expression.

use hpm_package::HoudiniRange;

const OPERATORS: &[&str] = &[">=", "<=", ">", "<", "==", "=", "^", "~", ""];
const OPERANDS: &[&str] = &[
    "21", "21.0", "20.5", "20.5.445", "21.0.0", "0.5", "0.0.3", "22.00",
];

mod matches_version_model {
    use super::*;
    use proptest::prelude::*;
    use std::cmp::Ordering;

    /// Houdini's rule, measured with hconfig on 22.0.368: a version is
    /// compared with an operand over the operand's components only, so
    /// 22.0.368 equals '22' and '22.0', is greater than '21', and less than
    /// '22.1'. An unknown build is the highest build of its line.
    fn compare(version: [u64; 3], operand: &[u64]) -> Ordering {
        version[..operand.len()].cmp(operand)
    }

    fn parts(v: &str) -> Vec<u64> {
        v.split('.').map(|s| s.parse().unwrap()).collect()
    }

    /// The comparisons one comma-separated clause stands for.
    fn clause(text: &str) -> Vec<(&'static str, Vec<u64>)> {
        let text = text.trim();
        for op in [">=", "<=", "==", ">", "<"] {
            if let Some(v) = text.strip_prefix(op) {
                return vec![(op, parts(v.trim()))];
            }
        }
        if let Some(v) = text.strip_prefix('=') {
            return vec![("==", parts(v.trim()))];
        }
        let (tilde, v) = match text.strip_prefix('~') {
            Some(v) => (true, parts(v.trim())),
            None => (false, parts(text.strip_prefix('^').unwrap_or(text).trim())),
        };
        // Caret: below the next change of the leftmost non-zero component
        // (or of the last one given); tilde: below the next minor (or major
        // when only a major is given).
        let upper = if tilde {
            match v.as_slice() {
                [m] => vec![m + 1],
                [m, n, ..] => vec![*m, n + 1],
                _ => unreachable!(),
            }
        } else {
            let i = v.iter().position(|&x| x != 0).unwrap_or(v.len() - 1);
            let mut u = v[..=i].to_vec();
            u[i] += 1;
            u
        };
        vec![(">=", v), ("<", upper)]
    }

    fn model(range: &str, version: [u64; 3]) -> bool {
        range.split(',').flat_map(clause).all(|(op, operand)| {
            let o = compare(version, &operand);
            match op {
                ">=" => o != Ordering::Less,
                "<=" => o != Ordering::Greater,
                ">" => o == Ordering::Greater,
                "<" => o == Ordering::Less,
                "==" => o == Ordering::Equal,
                _ => unreachable!(),
            }
        })
    }

    fn range() -> impl Strategy<Value = String> {
        let one = (
            prop::sample::select(OPERATORS),
            prop::sample::select(OPERANDS),
        )
            .prop_map(|(op, v)| format!("{op}{v}"));
        prop_oneof![
            one.clone(),
            (one.clone(), one).prop_map(|(a, b)| format!("{a}, {b}")),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        /// `matches_version` answers what Houdini's evaluation of the
        /// emitted `enable` would.
        #[test]
        fn matches_version_agrees_with_houdini(
            text in range(),
            major in 0u32..24,
            minor in 0u32..7,
            build in prop::option::of(prop::sample::select(vec![0u32, 1, 445, 1000])),
        ) {
            let Ok(r) = HoudiniRange::parse(&text) else { return Ok(()) };
            let version = [major as u64, minor as u64, build.map_or(u64::MAX, u64::from)];
            prop_assert_eq!(
                r.matches_version(major, minor, build),
                model(&text, version),
                "{} against {}.{}.{:?}", text, major, minor, build
            );
        }
    }

    /// The case that motivated the model: `==21` is a legal range that
    /// semver cannot parse, and the old fallback answered "matches" for any
    /// version.
    #[test]
    fn double_equals_matches_only_its_line() {
        let r = HoudiniRange::parse("==21").unwrap();
        assert!(r.matches_version(21, 0, Some(445)));
        assert!(!r.matches_version(22, 0, None));
    }
}
