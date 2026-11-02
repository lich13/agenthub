use std::{fs, path::PathBuf};

pub(super) fn source_line_count(relative: &str) -> usize {
    fs::read_to_string(src_path(relative))
        .expect("source file should be readable")
        .lines()
        .count()
}

fn src_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative)
}
