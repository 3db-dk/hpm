//! Where each kind of invalid `hpm.toml` is rejected (at parse or at
//! `validate()`) and with what message, over a corpus of the fields that
//! carry their own grammar: the package version, Python tags, platform keys
//! and script selectors.

use hpm_package::parse_manifest_str;

const HEAD: &str = "[package]\npath = \"acme/demo\"\nname = \"Demo\"\n";

const CASES: &[(&str, &str)] = &[
    ("prerelease version", "version = \"1.0.0-beta.1\"\n"),
    ("two-part version", "version = \"1.0\"\n"),
    ("word version", "version = \"latest\"\n"),
    (
        "runtime python ok",
        "version = \"1.0.0\"\n[runtime.X]\nmethod = \"set\"\nvalue = [{ when = { python = \"python3.11\" }, set = \"a\" }]\n",
    ),
    (
        "runtime python bad",
        "version = \"1.0.0\"\n[runtime.X]\nmethod = \"set\"\nvalue = [{ when = { python = \"3.x\" }, set = \"a\" }]\n",
    ),
    (
        "runtime python empty",
        "version = \"1.0.0\"\n[runtime.X]\nmethod = \"set\"\nvalue = [{ when = { python = \"\" }, set = \"a\" }]\n",
    ),
    (
        "script python bad",
        "version = \"1.0.0\"\n[scripts]\nbuild = { cmd = \"b\", python = \"three\" }\n",
    ),
    (
        "stage platform undeclared",
        "version = \"1.0.0\"\n[stage.platform.linux-x86_64]\nplace = [{ from = \"a\", to = \"b\" }]\n",
    ),
    (
        "stage platform unknown",
        "version = \"1.0.0\"\n[compat]\nplatforms = [\"linux-x86_64\"]\n[stage.platform.beos-x86]\nplace = [{ from = \"a\", to = \"b\" }]\n",
    ),
    (
        "profile platform undeclared",
        "version = \"1.0.0\"\n[stage.profile.dev.platform.macos-aarch64]\nplace = [{ from = \"a\", to = \"b\" }]\n",
    ),
    (
        "operator platform undeclared",
        "version = \"1.0.0\"\n[[operators]]\nkind = \"dso\"\ntype_name = \"a::b\"\ncategory = \"Sop\"\nsource = { windows-x86_64 = \"dso/a.dll\" }\n",
    ),
    (
        "operator platform unknown",
        "version = \"1.0.0\"\n[compat]\nplatforms = [\"windows-x86_64\"]\n[[operators]]\nkind = \"dso\"\ntype_name = \"a::b\"\ncategory = \"Sop\"\nsource = { amiga = \"dso/a.dll\" }\n",
    ),
    (
        "operator platform ok",
        "version = \"1.0.0\"\n[compat]\nplatforms = [\"windows-x86_64\"]\n[[operators]]\nkind = \"dso\"\ntype_name = \"a::b\"\ncategory = \"Sop\"\nsource = { windows-x86_64 = \"dso/a.dll\" }\n",
    ),
    (
        "script when houdini",
        "version = \"1.0.0\"\n[scripts]\nb = { cmd = [{ when = { houdini = \">=21\" }, set = \"x\" }] }\n",
    ),
    (
        "script when install_source",
        "version = \"1.0.0\"\n[scripts]\nb = { cmd = [{ when = { install_source = \"dev\" }, set = \"x\" }] }\n",
    ),
    (
        "script when unknown os",
        "version = \"1.0.0\"\n[scripts]\nb = { cmd = [{ when = { os = \"beos\" }, set = \"x\" }] }\n",
    ),
    (
        "script empty cmd list",
        "version = \"1.0.0\"\n[scripts]\nb = { cmd = [] }\n",
    ),
    (
        "script when os ok",
        "version = \"1.0.0\"\n[scripts]\nb = { cmd = [{ when = { os = \"windows\" }, set = \"b.bat\" }, { set = \"./b.sh\" }] }\n",
    ),
];

#[test]
fn where_invalid_manifests_are_rejected() {
    let mut out = String::new();
    for (name, body) in CASES {
        let outcome = match parse_manifest_str(&format!("{HEAD}{body}")) {
            Err(e) => format!("parse error: {}", e.message()),
            Ok(m) => match m.validate() {
                Ok(()) => "ok".to_string(),
                Err(e) => format!("validate error: {e}"),
            },
        };
        out += &format!("{name}: {outcome}\n");
    }
    insta::assert_snapshot!(out);
}
