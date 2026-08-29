//! include/plotui.h is committed so consumers (the Go package's cgo build)
//! never need cbindgen installed. This test keeps it fresh: it regenerates
//! the header in memory and diffs it against the committed file. To update
//! after changing the ABI:
//!
//!     PLOTUI_REGEN_HEADER=1 cargo test -p plotui-ffi header_is_fresh

use std::path::PathBuf;

#[test]
fn header_is_fresh() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let header_path = crate_dir.join("include/plotui.h");

    let mut generated = Vec::new();
    cbindgen::generate(&crate_dir).expect("cbindgen failed").write(&mut generated);
    let generated = String::from_utf8(generated).expect("header is UTF-8");

    if std::env::var("PLOTUI_REGEN_HEADER").is_ok() {
        std::fs::create_dir_all(header_path.parent().unwrap()).unwrap();
        std::fs::write(&header_path, &generated).unwrap();
        return;
    }

    let committed = std::fs::read_to_string(&header_path)
        .expect("include/plotui.h missing — regenerate it (see this test's docs)");
    assert_eq!(
        committed, generated,
        "include/plotui.h is stale — regenerate with PLOTUI_REGEN_HEADER=1 \
         cargo test -p plotui-ffi header_is_fresh"
    );
}
