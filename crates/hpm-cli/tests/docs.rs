//! The documentation in `docs/` is checked against the code it describes.
//!
//! Nothing compiles prose, so these tests cover what drifts silently: a page
//! missing from one of the two site indexes, a link to a heading that was
//! renamed, a manifest example the parser no longer accepts, a flag `cli.md`
//! documents that clap does not have (or the reverse), and a type or function
//! named in the contributor pages that no longer exists.

use clap::CommandFactory;
use hpm_cli::Cli;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

/// Every `docs/*.md`, as `(file name, content)`.
fn pages() -> BTreeMap<String, String> {
    let dir = repo_root().join("docs");
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(&dir).expect("docs/ is readable") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|e| e == "md") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.insert(name, fs::read_to_string(&path).expect("page is readable"));
        }
    }
    out
}

/// The lines of a page outside fenced code blocks, with 1-based numbers.
fn prose_lines(content: &str) -> Vec<(usize, &str)> {
    let mut in_code = false;
    let mut out = Vec::new();
    for (i, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            out.push((i + 1, line));
        }
    }
    out
}

/// Fenced blocks tagged `lang`, with the line each starts on.
fn code_blocks(content: &str, lang: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current: Option<(usize, String)> = None;
    let mut in_other = false;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((_, body)) = current.as_mut() {
            if trimmed.starts_with("```") {
                out.push(current.take().unwrap());
            } else {
                body.push_str(line);
                body.push('\n');
            }
        } else if in_other {
            in_other = !trimmed.starts_with("```");
        } else if let Some(tag) = trimmed.strip_prefix("```") {
            if tag.trim() == lang {
                current = Some((i + 1, String::new()));
            } else {
                in_other = true;
            }
        }
    }
    out
}

/// The anchor both mdBook and VitePress derive from a heading.
fn heading_anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

fn anchors(content: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, line) in prose_lines(content) {
        if let Some(rest) = line.strip_prefix('#') {
            out.insert(heading_anchor(rest.trim_start_matches('#')));
        }
        let mut rest = line;
        while let Some(at) = rest.find("<a id=\"") {
            rest = &rest[at + 7..];
            if let Some(end) = rest.find('"') {
                out.insert(rest[..end].to_string());
            }
        }
    }
    out
}

/// Targets of `[text](target)` links on a line.
fn link_targets(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        if let Some(end) = rest.find(')') {
            out.push(&rest[..end]);
        }
    }
    out
}

/// Pages in the order `SUMMARY.md` lists them.
fn summary_pages() -> Vec<String> {
    let summary = fs::read_to_string(repo_root().join("docs/SUMMARY.md")).expect("SUMMARY.md");
    summary
        .lines()
        .flat_map(link_targets)
        .map(str::to_string)
        .collect()
}

#[test]
fn both_site_indexes_list_every_page() {
    let on_disk: BTreeSet<String> = pages()
        .into_keys()
        .filter(|name| name != "SUMMARY.md")
        .collect();
    let summary: BTreeSet<String> = summary_pages().into_iter().collect();

    let manifest: toml::Table = fs::read_to_string(repo_root().join("docs/manifest.toml"))
        .expect("manifest.toml")
        .parse()
        .expect("docs/manifest.toml is valid TOML");
    let vitepress: BTreeSet<String> = manifest["pages"]
        .as_array()
        .expect("[[pages]]")
        .iter()
        .map(|page| page["file"].as_str().expect("file").to_string())
        .collect();

    assert_eq!(
        summary, on_disk,
        "docs/SUMMARY.md (hpm.readthedocs.io) must list exactly the pages in docs/"
    );
    assert_eq!(
        vitepress, on_disk,
        "docs/manifest.toml (docs.tumbletrove.com) must list exactly the pages in docs/"
    );
}

#[test]
fn internal_links_resolve() {
    let pages = pages();
    let anchors: BTreeMap<&str, BTreeSet<String>> = pages
        .iter()
        .map(|(name, content)| (name.as_str(), anchors(content)))
        .collect();

    let mut broken = Vec::new();
    for (name, content) in &pages {
        for (line_no, line) in prose_lines(content) {
            for target in link_targets(line) {
                if target.contains("://") || target.starts_with("mailto:") {
                    continue;
                }
                let (file, fragment) = target.split_once('#').unwrap_or((target, ""));
                let file = if file.is_empty() { name.as_str() } else { file };
                match anchors.get(file) {
                    None => broken.push(format!("{name}:{line_no}: no page {file}")),
                    Some(known) if !fragment.is_empty() && !known.contains(fragment) => broken
                        .push(format!(
                            "{name}:{line_no}: no heading #{fragment} in {file}"
                        )),
                    Some(_) => {}
                }
            }
        }
    }
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}

/// Top-level tables of `hpm.toml`. A `toml` block made only of these is a
/// manifest example and must load as one.
const MANIFEST_SECTIONS: &[&str] = &[
    "package",
    "compat",
    "dependencies",
    "python_dependencies",
    "runtime",
    "scripts",
    "stage",
    "operators",
    "registries",
];

#[test]
fn toml_examples_parse() {
    let mut pages = pages();
    pages.insert(
        "../README.md".to_string(),
        fs::read_to_string(repo_root().join("README.md")).expect("README.md"),
    );

    let mut failures = Vec::new();
    let mut manifests = 0;
    for (name, content) in &pages {
        for (line_no, block) in code_blocks(content, "toml") {
            let table = match block.parse::<toml::Table>() {
                Ok(table) => table,
                Err(err) => {
                    failures.push(format!("{name}:{line_no}: not TOML: {err}"));
                    continue;
                }
            };
            let is_manifest = !table.is_empty()
                && table
                    .keys()
                    .all(|key| MANIFEST_SECTIONS.contains(&key.as_str()));
            if !is_manifest {
                continue; // config.toml, hpm.lock and other files
            }
            // A fragment shows one section; give it the identity it leaves out.
            let complete = if table.contains_key("package") {
                block
            } else {
                // `[package]` must come first: keys before the first table
                // header would otherwise land in it.
                format!(
                    "[package]\npath = \"docs/example\"\nname = \"Example\"\nversion = \"1.0.0\"\n\n{block}"
                )
            };
            manifests += 1;
            if let Err(err) = hpm_package::parse_manifest_str(&complete) {
                failures.push(format!("{name}:{line_no}: not a valid hpm.toml: {err}"));
            }
        }
    }
    assert!(manifests > 20, "found only {manifests} manifest examples");
    assert!(
        failures.is_empty(),
        "TOML examples that do not parse:\n{}",
        failures.join("\n")
    );
}

/// Long flags of a command and all its subcommands.
fn long_flags(command: &clap::Command, out: &mut BTreeSet<String>) {
    for arg in command.get_arguments() {
        if arg.is_hide_set() {
            continue;
        }
        if let Some(long) = arg.get_long() {
            out.insert(format!("--{long}"));
        }
    }
    for sub in command.get_subcommands() {
        long_flags(sub, out);
    }
}

/// `--flag` tokens in a text.
fn mentioned_flags(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let starts = bytes[i] == b'-'
            && bytes[i + 1] == b'-'
            && bytes[i + 2].is_ascii_lowercase()
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-'));
        if starts {
            let end = (i + 2..bytes.len())
                .find(|&j| !(bytes[j].is_ascii_alphanumeric() || bytes[j] == b'-'))
                .unwrap_or(bytes.len());
            out.insert(text[i..end].trim_end_matches('-').to_string());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// Flags `cli.md` mentions that belong to other programs (`cmake --build`,
/// `git`, ...), not to hpm.
const FOREIGN_FLAGS: &[&str] = &["--build", "--config"];

#[test]
fn cli_reference_matches_clap() {
    let mut real = BTreeSet::new();
    long_flags(&Cli::command(), &mut real);
    real.insert("--help".to_string());
    real.insert("--version".to_string());

    let cli_md = &pages()["cli.md"];
    let documented = mentioned_flags(cli_md);

    let unknown: Vec<_> = documented
        .iter()
        .filter(|flag| !real.contains(*flag) && !FOREIGN_FLAGS.contains(&flag.as_str()))
        .collect();
    let undocumented: Vec<_> = real
        .difference(&documented)
        .filter(|flag| !matches!(flag.as_str(), "--help" | "--version"))
        .collect();

    assert!(
        unknown.is_empty(),
        "docs/cli.md mentions flags hpm does not have: {unknown:?}"
    );
    assert!(
        undocumented.is_empty(),
        "docs/cli.md does not mention these flags: {undocumented:?}"
    );
}

/// Whether `text` is an identifier worth checking: `CamelCase` with at least
/// two words, or `snake_case` with at least one underscore.
fn looks_like_identifier(text: &str) -> bool {
    let camel = text.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && text.chars().all(|c| c.is_ascii_alphanumeric())
        && text.chars().skip(1).any(|c| c.is_ascii_uppercase())
        && text.chars().any(|c| c.is_ascii_lowercase());
    let snake = text.contains('_')
        && text.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    camel || snake
}

fn rust_sources(dir: &Path, out: &mut String) {
    for entry in fs::read_dir(dir).expect("source dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                rust_sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs" || e == "toml") {
            out.push_str(&fs::read_to_string(&path).unwrap_or_default());
            out.push('\n');
        }
    }
}

/// Names the contributor pages use that are not Rust items in this
/// workspace: manifest keys, JSON fields, file names, external APIs.
const NOT_CODE: &[&str] = &[
    // `AssetKind` variants as serde spells them in the asset index.
    "hda_operator",
    "dso_operator",
];

#[test]
fn contributor_pages_name_things_that_exist() {
    let mut sources = String::new();
    rust_sources(&repo_root().join("crates"), &mut sources);
    let words: BTreeSet<&str> = sources
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .collect();

    let pages = pages();
    let mut missing = Vec::new();
    for name in ["architecture.md", "api-overview.md", "testing.md"] {
        for (line_no, line) in prose_lines(&pages[name]) {
            // Odd-numbered pieces of a line split on backticks are code spans.
            for span in line.split('`').skip(1).step_by(2) {
                let item = span.trim_end_matches("()");
                let last = item.rsplit("::").next().unwrap_or(item);
                if looks_like_identifier(last) && !words.contains(last) && !NOT_CODE.contains(&last)
                {
                    missing.push(format!("{name}:{line_no}: `{span}`"));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "contributor docs name items that are not in crates/ (rename them, or \
         add to NOT_CODE if they are not Rust items):\n{}",
        missing.join("\n")
    );
}
