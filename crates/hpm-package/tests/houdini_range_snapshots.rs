//! Characterization of everything derived from a `[compat].houdini` range,
//! over every operator, partial and full operands, and comma-joined forms:
//! the `enable` expression Houdini evaluates (must never change), its
//! SideFX-hpackage form, the lower bound used for Python ABI selection, and
//! whether the range bounds above.

use hpm_package::HoudiniRange;

const OPERATORS: &[&str] = &[">=", "<=", ">", "<", "==", "=", "^", "~", ""];
const OPERANDS: &[&str] = &[
    "21", "21.0", "20.5", "20.5.445", "21.0.0", "0.5", "0.0.3", "22.00",
];
const COMPOUND: &[&str] = &[
    ">=20.5, <22",
    ">21, <=22.5",
    "^21, <21.5",
    "~20.5, >=20.5.445",
    "<22, >=20.5",
    "=21, <21.5",
    " >= 21 ,  < 22 ",
    ">=21,",
    "",
    "!=21",
    "21.x",
    ">=21.0.0.1",
];

fn row(text: &str) -> String {
    match HoudiniRange::parse(text) {
        Err(e) => format!("{text:?}\n  parse: error: {e}\n"),
        Ok(r) => format!(
            "{text:?}\n  enable: {}\n  hpackage: {}\n  lower_bound: {:?}\n  has_upper_bound: {}\n",
            r.to_enable_expression(),
            match r.to_enable_expression_hpackage() {
                Ok(s) => s,
                Err(e) => format!(
                    "error: {}",
                    e.to_string().chars().take(60).collect::<String>()
                ),
            },
            r.lower_bound(),
            r.has_upper_bound(),
        ),
    }
}

#[test]
fn houdini_range_derivations() {
    let mut out = String::new();
    for op in OPERATORS {
        for v in OPERANDS {
            out += &row(&format!("{op}{v}"));
        }
    }
    for text in COMPOUND {
        out += &row(text);
    }
    insta::assert_snapshot!(out);
}
