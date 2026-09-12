mod common;

use common::{tree, write};
use sync_rs::sync::{override_path, sync_directory, sync_relative, Rules};
use tempfile::tempdir;

#[test]
fn override_paths_normalize_and_stay_inside() {
    assert_eq!(override_path("./data/").unwrap(), "data");
    assert_eq!(override_path("sub//data").unwrap(), "sub/data");
    assert_eq!(override_path(".env").unwrap(), ".env");
    for outside in ["../shared", "/abs/path", ".", "", "sub/../.."] {
        assert!(override_path(outside).is_err(), "{outside:?} accepted");
    }
}

#[test]
fn excluded_override_path_survives_main_sync() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(src.join("top"), "");
    write(dst.join("data/old"), "");
    write(dst.join("stale"), "");
    let source = format!("{}/", src.display());

    sync_directory(
        &source,
        dst.to_str().unwrap(),
        Rules::Stream(&[b"- /data".to_vec()]),
        true,
    )
    .unwrap();
    assert_eq!(tree(&dst), ["data/old", "top"]);

    sync_directory(&source, dst.to_str().unwrap(), Rules::Stream(&[]), true).unwrap();
    assert_eq!(tree(&dst), ["top"]);
}

#[test]
fn relative_sync_places_the_path_and_scopes_deletion() {
    let tmp = tempdir().unwrap();
    let src = tmp.path().join("src");
    let dst = tmp.path().join("dst");
    write(src.join("sub/data/f"), "");
    write(src.join("sub/data/x.tmp"), "");
    write(src.join(".env"), "");
    write(dst.join("sub/data/stale"), "");
    write(dst.join("sub/other/o"), "");
    write(dst.join("top"), "");
    let rules = [String::from("- *.tmp")];

    sync_relative(
        &src,
        "sub/data",
        dst.to_str().unwrap(),
        Rules::Args(&rules),
        false,
    )
    .unwrap();
    assert_eq!(
        tree(&dst),
        ["sub/data/f", "sub/data/stale", "sub/other/o", "top"]
    );

    sync_relative(
        &src,
        "sub/data",
        dst.to_str().unwrap(),
        Rules::Args(&rules),
        true,
    )
    .unwrap();
    assert_eq!(tree(&dst), ["sub/data/f", "sub/other/o", "top"]);

    sync_relative(&src, ".env", dst.to_str().unwrap(), Rules::Args(&[]), true).unwrap();
    assert_eq!(tree(&dst), [".env", "sub/data/f", "sub/other/o", "top"]);
}
