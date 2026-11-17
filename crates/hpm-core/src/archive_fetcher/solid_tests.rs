//! Solid `.tar.zst` archives: a round trip through the extractor, and an
//! opt-in benchmark that repacks a real release zip both ways.

use super::compute_directory_checksum_sync;
use super::extract::extract_archive_sync;
use crate::packer::{EntrySource, PlannedEntry, SolidParams, write_tar_zst, write_zip};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Unpack `zip_path` into `dir`, returning (archive name, file path) in the
/// zip's own entry order.
fn unzip_to(zip_path: &Path, dir: &Path) -> Vec<(String, PathBuf)> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(zip_path).unwrap()).unwrap();
    let mut out = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if f.is_dir() {
            continue;
        }
        let name = f.name().unwrap().to_string();
        let path = dir.join(&name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut w = std::fs::File::create(&path).unwrap();
        std::io::copy(&mut f, &mut w).unwrap();
        out.push((name, path));
    }
    out
}

fn planned(files: &[(String, PathBuf)]) -> Vec<PlannedEntry<'_>> {
    files
        .iter()
        .map(|(name, path)| PlannedEntry {
            name: name.clone(),
            source: EntrySource::File(path, 0o644),
        })
        .collect()
}

/// An hpackage-layout tree written as zip and as tar.zst installs to the
/// same tree (same layout resolution: root json skipped, `{slug}/` stripped).
#[test]
fn a_solid_archive_installs_the_same_tree_as_the_zip() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let mut files = Vec::new();
    let mut add = |name: &str, bytes: &[u8]| {
        let p = src.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        files.push((name.to_string(), p));
    };
    let graph = "node { name = \"spline\" }\n".repeat(2000);
    add("demo/apex/spline-1.0.apex", graph.as_bytes());
    add(
        "demo/apex/spline-1.1.apex",
        format!("{graph}// 1.1\n").as_bytes(),
    );
    add(
        "demo/a/very/long/directory/name/that/goes/past/one/hundred/bytes/of/tar/ustar/path/field/file.txt",
        b"long name",
    );
    add("demo/empty.txt", b"");
    add("demo.json", b"{}");

    let entries = planned(&files);
    let zip_path = tmp.path().join("p.zip");
    let zst_path = tmp.path().join("p.tar.zst");
    write_zip(&entries, &zip_path).unwrap();
    write_tar_zst(&entries, &zst_path, SolidParams::default()).unwrap();

    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    extract_archive_sync(&zip_path, &a).unwrap();
    extract_archive_sync(&zst_path, &b).unwrap();
    assert!(b.join("apex/spline-1.1.apex").is_file());
    assert!(!b.join("demo.json").exists(), "root json is skipped");
    assert_eq!(
        compute_directory_checksum_sync(&a).unwrap(),
        compute_directory_checksum_sync(&b).unwrap()
    );

    // Reproducible: a second pack is byte-identical, also on another
    // thread count.
    let again = tmp.path().join("again.tar.zst");
    write_tar_zst(
        &entries,
        &again,
        SolidParams {
            workers: 1,
            ..SolidParams::default()
        },
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&zst_path).unwrap(),
        std::fs::read(&again).unwrap()
    );
}

/// Repack a real release zip as zip and as tar.zst and time both installs.
///
/// `HPM_SOLID_BENCH_ZIP=<release.zip> HPM_SOLID_BENCH_DIR=<scratch dir>
/// cargo test --release -p hpm-core solid_bench -- --ignored --nocapture`
#[test]
#[ignore = "benchmark; needs HPM_SOLID_BENCH_ZIP"]
fn solid_bench() {
    let zip_in = PathBuf::from(std::env::var("HPM_SOLID_BENCH_ZIP").unwrap());
    let work = PathBuf::from(std::env::var("HPM_SOLID_BENCH_DIR").unwrap());
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let mb = |p: &Path| std::fs::metadata(p).unwrap().len() as f64 / 1e6;

    let t = Instant::now();
    let files = unzip_to(&zip_in, &work.join("src"));
    let raw: u64 = files
        .iter()
        .map(|(_, p)| std::fs::metadata(p).unwrap().len())
        .sum();
    println!(
        "input {} : {} files, {:.1} MB unpacked, {:.1} MB zip ({:.1}s to unzip)",
        zip_in.display(),
        files.len(),
        raw as f64 / 1e6,
        mb(&zip_in),
        t.elapsed().as_secs_f64()
    );
    let entries = planned(&files);

    let mut variants: Vec<(String, PathBuf, f64)> = Vec::new();
    let t = Instant::now();
    let p = work.join("repack.zip");
    write_zip(&entries, &p).unwrap();
    variants.push((
        "zip (deflate, hpm pack)".into(),
        p,
        t.elapsed().as_secs_f64(),
    ));

    let workers = SolidParams::default().workers;
    let levels: Vec<(i32, u32)> = std::env::var("HPM_SOLID_BENCH_PARAMS")
        .unwrap_or_else(|_| "19:27,19:30,9:27".into())
        .split(',')
        .map(|s| {
            let (l, w) = s.split_once(':').unwrap();
            (l.parse().unwrap(), w.parse().unwrap())
        })
        .collect();
    for (level, window_log) in levels {
        let t = Instant::now();
        let p = work.join(format!("repack-l{level}-w{window_log}.tar.zst"));
        write_tar_zst(
            &entries,
            &p,
            SolidParams {
                level,
                window_log,
                workers,
            },
        )
        .unwrap();
        variants.push((
            format!("tar.zst level {level}, window 2^{window_log}"),
            p,
            t.elapsed().as_secs_f64(),
        ));
    }

    let mut reference: Option<String> = None;
    println!("| archive | MB | pack s | install (extract) s | tree |");
    println!("|---|---|---|---|---|");
    for (i, (label, path, pack_s)) in variants.iter().enumerate() {
        let dest = work.join(format!("x{i}"));
        let t = Instant::now();
        extract_archive_sync(path, &dest).unwrap();
        let unpack_s = t.elapsed().as_secs_f64();
        let sum = compute_directory_checksum_sync(&dest).unwrap();
        let same = match &reference {
            None => {
                reference = Some(sum);
                "reference"
            }
            Some(r) if *r == sum => "identical",
            Some(_) => "DIFFERS",
        };
        println!(
            "| {label} | {:.1} | {pack_s:.1} | {unpack_s:.2} | {same} |",
            mb(path)
        );
        std::fs::remove_dir_all(&dest).unwrap();
    }
}
