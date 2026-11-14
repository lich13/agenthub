use super::{DesktopError, NativeReference};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "agenthub-desktop-references-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).expect("synthetic reference fixture directory is created");
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("synthetic reference fixture directory is private");
        Self(root)
    }

    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).expect("synthetic reference fixture file is written");
        path
    }

    fn sparse_file(&self, name: &str, size: u64, prefix: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, prefix).expect("synthetic sparse reference file is created");
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("synthetic sparse reference file can be extended")
            .set_len(size)
            .expect("synthetic sparse reference file size is set");
        path
    }

    fn private_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .expect("synthetic private reference file is created");
        file.write_all(bytes)
            .expect("synthetic private reference file contents are written");
        file.sync_all()
            .expect("synthetic private reference file is synced");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn metadata_tuple(path: &Path) -> (u64, u64, u64, i64, i64, u32, u32, u32) {
    let metadata = fs::metadata(path).expect("synthetic reference metadata exists");
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.uid(),
        metadata.gid(),
        metadata.mode() & 0o7777,
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn serialized_identity(reference: &NativeReference) -> Value {
    serde_json::to_value(reference)
        .expect("synthetic native reference serializes")
        .get("identity")
        .cloned()
        .expect("synthetic file reference includes identity")
}

fn restore_mtime(path: &Path, seconds: i64, nanos: i64) {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .expect("synthetic reference path has no NUL byte");
    let times = [
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
        libc::timespec {
            tv_sec: seconds,
            tv_nsec: nanos,
        },
    ];
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(result, 0, "synthetic reference mtime is restored");
}

fn remove_content_proof(mut encoded: Value) -> Value {
    encoded
        .get_mut("identity")
        .and_then(Value::as_object_mut)
        .expect("serialized file identity is an object")
        .remove("content_proof");
    encoded
}

#[test]
fn selected_files_are_classified_from_content_signatures() {
    let fixture = Fixture::new();
    let regular_path = fixture.file("fixture-data.bin", b"synthetic attachment contents");
    let image_path = fixture.file(
        "fixture-image.data",
        b"\x89PNG\r\n\x1a\nsynthetic image bytes",
    );

    let regular =
        NativeReference::selected_file(regular_path).expect("synthetic regular file is selected");
    let image =
        NativeReference::selected_file(image_path).expect("synthetic PNG signature is selected");

    assert_eq!(regular.kind(), "file");
    assert_eq!(image.kind(), "image");
    regular
        .validate()
        .expect("selected regular file keeps its identity");
    image.validate().expect("selected image keeps its identity");
}

#[test]
fn selected_file_detailed_returns_specific_safe_errors_and_legacy_classification() {
    let fixture = Fixture::new();
    let oversized_image = fixture.sparse_file(
        "fixture-oversized-image.png",
        20 * 1024 * 1024 + 1,
        b"\x89PNG\r\n\x1a\n",
    );
    let oversized_error = match NativeReference::selected_file_detailed(oversized_image.clone()) {
        Ok(_) => panic!("oversized synthetic image is rejected"),
        Err(error) => error,
    };
    assert_eq!(
        oversized_error,
        (
            DesktopError::InvalidInput,
            "图片超过 20MiB，请选择较小的图片"
        )
    );
    assert!(matches!(
        NativeReference::selected_file(oversized_image),
        Err(DesktopError::InvalidInput)
    ));

    let directory_error = match NativeReference::selected_file_detailed(fixture.0.clone()) {
        Ok(_) => panic!("synthetic fixture directory is rejected as a file"),
        Err(error) => error,
    };
    assert_eq!(
        directory_error,
        (DesktopError::InvalidInput, "仅支持普通文件，不能添加目录")
    );
    assert!(matches!(
        NativeReference::selected_file(fixture.0.clone()),
        Err(DesktopError::InvalidInput)
    ));

    let unavailable = fixture.0.join("fixture-disappeared.bin");
    let unavailable_error = match NativeReference::selected_file_detailed(unavailable.clone()) {
        Ok(_) => panic!("a missing synthetic file is rejected"),
        Err(error) => error,
    };
    assert_eq!(
        unavailable_error,
        (DesktopError::InvalidInput, "无法读取所选文件，请重新选择")
    );
    assert!(matches!(
        NativeReference::selected_file(unavailable),
        Err(DesktopError::InvalidInput)
    ));
}

#[test]
fn selected_symlink_is_resolved_but_replacing_its_target_expires_the_reference() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let original = fixture.file("fixture-symlink-target.bin", b"synthetic original contents");
    let link = fixture.0.join("fixture-link.bin");
    symlink(&original, &link).expect("synthetic selected-file symlink is created");

    let reference = NativeReference::selected_file_detailed(link)
        .expect("legacy selection resolves the synthetic symlink target");
    let NativeReference::File { path, image, .. } = &reference else {
        panic!("selected symlink target remains a file reference");
    };
    assert!(!image);
    assert_eq!(
        path,
        &original.canonicalize().expect("fixture target resolves")
    );

    let replacement = fixture.file("fixture-replacement.bin", b"synthetic replacement contents");
    fs::remove_file(&original).expect("synthetic selected target is removed");
    symlink(&replacement, &original).expect("synthetic target is replaced by a symlink");
    assert_eq!(
        reference.validate().unwrap_err(),
        DesktopError::RequestExpired
    );
}

#[test]
fn app_references_reject_file_paths_and_unknown_schemes() {
    for path in [
        "file:///fixture/app",
        "https://example.invalid/app",
        "unknown:fixture",
    ] {
        assert!(matches!(
            NativeReference::app("fixture-app".into(), path.into()),
            Err(DesktopError::InvalidInput)
        ));
    }

    NativeReference::app("fixture-app".into(), "app://fixture-connector".into())
        .expect("native app connector URI is accepted")
        .validate()
        .expect("native app connector URI remains valid");
}

#[test]
fn captured_skill_rejects_truncation_and_symlink_replacement() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();

    let truncated_path = fixture.file(
        "truncated-skill.md",
        b"synthetic skill body before truncation",
    );
    let captured_truncated =
        NativeReference::skill("fixture-truncated-skill".into(), truncated_path.clone())
            .expect("synthetic skill reference is accepted")
            .capture()
            .expect("synthetic skill identity is captured");
    fs::write(&truncated_path, b"x").expect("synthetic skill fixture is truncated");
    assert_eq!(
        captured_truncated.validate().unwrap_err(),
        DesktopError::RequestExpired
    );

    let replaced_path = fixture.file("replaced-skill.md", b"synthetic original skill body");
    let replacement_target = fixture.file("replacement-target.md", b"synthetic replacement body");
    let captured_replaced =
        NativeReference::skill("fixture-replaced-skill".into(), replaced_path.clone())
            .expect("synthetic skill reference is accepted")
            .capture()
            .expect("synthetic skill identity is captured");
    fs::remove_file(&replaced_path).expect("captured skill fixture is removed");
    symlink(&replacement_target, &replaced_path)
        .expect("captured path is replaced with a fixture symlink");
    assert_eq!(
        captured_replaced.validate().unwrap_err(),
        DesktopError::RequestExpired
    );
}

#[test]
fn small_files_and_boundary_images_carry_a_content_proof_but_large_files_do_not() {
    const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

    let fixture = Fixture::new();
    let regular_path = fixture.private_file("proofed-regular.bin", b"proofed fixture bytes");
    let regular = NativeReference::selected_file(regular_path)
        .expect("small synthetic regular file is selected");
    let identity = serialized_identity(&regular);
    assert_eq!(
        identity["content_proof"]["sha256"],
        sha256_hex(b"proofed fixture bytes")
    );
    assert_eq!(
        identity["content_proof"]["uid"],
        serde_json::json!(fs::metadata(match &regular {
            NativeReference::File { path, .. } => path,
            _ => unreachable!(),
        })
        .expect("proofed regular metadata exists")
        .uid())
    );
    assert!(identity["content_proof"]["gid"].is_u64());
    assert!(identity["content_proof"]["mode"].is_u64());

    let image_path =
        fixture.sparse_file("boundary-image.png", MAX_IMAGE_BYTES, b"\x89PNG\r\n\x1a\n");
    let image = NativeReference::selected_file(image_path)
        .expect("synthetic image at the exact 20 MiB limit is accepted");
    assert_eq!(image.kind(), "image");
    assert!(serialized_identity(&image)["content_proof"]["sha256"].is_string());

    let large_path = fixture.sparse_file(
        "large-regular.bin",
        MAX_IMAGE_BYTES + 1,
        b"synthetic large regular file",
    );
    let mut large = NativeReference::selected_file(large_path.clone())
        .expect("large synthetic regular file remains selectable");
    assert!(serialized_identity(&large).get("content_proof").is_none());
    large
        .validate()
        .expect("large synthetic regular file keeps strict metadata identity");
    let before = metadata_tuple(&large_path);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&large_path)
        .expect("large synthetic file can be rewritten");
    file.write_all(b"rewritten large regular file")
        .expect("large synthetic file rewrite succeeds");
    file.flush().expect("large synthetic rewrite is flushed");
    drop(file);
    restore_mtime(&large_path, before.3, before.4);
    if let NativeReference::File { identity, .. } = &mut large {
        identity.changed = fs::metadata(&large_path).unwrap().ctime() - 1;
    }
    assert_eq!(
        large.validate().unwrap_err(),
        DesktopError::RequestExpired,
        "large references without a content proof remain strict after a rewrite"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn xattr_only_change_keeps_a_small_file_reference_valid_with_content_proof() {
    let fixture = Fixture::new();
    let path = fixture.private_file("xattr-proofed.bin", b"xattr-only fixture bytes");
    let reference = NativeReference::selected_file(path.clone())
        .expect("small synthetic file is selected before xattr update");
    let before = metadata_tuple(&path);

    let status = std::process::Command::new("xattr")
        .args([
            "-w",
            "com.agenthub.fixture",
            "synthetic",
            path.to_str().unwrap(),
        ])
        .status()
        .expect("macOS xattr command is available");
    assert!(status.success(), "synthetic xattr update succeeds");

    let after = metadata_tuple(&path);
    assert_eq!(before.0, after.0, "xattr update keeps the device");
    assert_eq!(before.1, after.1, "xattr update keeps the inode");
    assert_eq!(before.2, after.2, "xattr update keeps the size");
    assert_eq!(before.3, after.3, "xattr update keeps the mtime seconds");
    assert_eq!(before.4, after.4, "xattr update keeps the mtime nanos");
    assert_eq!(before.5, after.5, "xattr update keeps the uid");
    assert_eq!(before.6, after.6, "xattr update keeps the gid");
    assert_eq!(before.7, after.7, "xattr update keeps the mode");
    reference
        .validate()
        .expect("xattr-only ctime change is accepted by a matching content proof");
}

#[test]
fn same_size_rewrite_with_restored_mtime_is_rejected_by_the_content_proof() {
    let fixture = Fixture::new();
    let path = fixture.private_file("rewritten-proofed.bin", b"AAAA synthetic bytes");
    let mut reference = NativeReference::selected_file(path.clone())
        .expect("small synthetic file is selected before rewrite");
    let before = metadata_tuple(&path);

    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("proofed file can be rewritten");
    file.write_all(b"BBBB synthetic bytes")
        .expect("same-size synthetic rewrite succeeds");
    file.flush().expect("synthetic rewrite is flushed");
    drop(file);
    restore_mtime(&path, before.3, before.4);

    // Filesystems may keep ctime during rapid writes; explicitly exercise a
    // stale ctime while retaining every other identity field and the old proof.
    if let NativeReference::File { identity, .. } = &mut reference {
        identity.changed = fs::metadata(&path).unwrap().ctime() - 1;
    }
    let after = metadata_tuple(&path);
    assert_eq!(before.0, after.0, "rewrite keeps the device");
    assert_eq!(before.1, after.1, "rewrite keeps the inode");
    assert_eq!(before.2, after.2, "rewrite keeps the size");
    assert_eq!(before.3, after.3, "rewrite restores mtime seconds");
    assert_eq!(before.4, after.4, "rewrite restores mtime nanos");
    assert_eq!(before.5, after.5, "rewrite keeps the uid");
    assert_eq!(before.6, after.6, "rewrite keeps the gid");
    assert_eq!(before.7, after.7, "rewrite keeps the mode");
    assert_eq!(
        reference.validate().unwrap_err(),
        DesktopError::RequestExpired,
        "same-size content changes do not become valid by restoring mtime"
    );
}

#[test]
fn replacing_an_inode_with_matching_content_expires_a_proofed_reference() {
    let fixture = Fixture::new();
    let path = fixture.private_file("inode-original.bin", b"same synthetic bytes");
    let reference =
        NativeReference::selected_file(path.clone()).expect("original synthetic file is selected");
    let original = metadata_tuple(&path);
    let replacement = fixture.private_file("inode-replacement.bin", b"same synthetic bytes");
    fs::set_permissions(&replacement, fs::Permissions::from_mode(original.7))
        .expect("replacement mode matches the original");
    restore_mtime(&replacement, original.3, original.4);
    fs::rename(&replacement, &path).expect("synthetic file is atomically replaced");

    let current = metadata_tuple(&path);
    assert_ne!(
        original.1, current.1,
        "replacement receives a different inode"
    );
    assert_eq!(original.2, current.2, "replacement keeps the file size");
    assert_eq!(original.3, current.3, "replacement keeps the mtime seconds");
    assert_eq!(original.4, current.4, "replacement keeps the mtime nanos");
    assert_eq!(
        reference.validate().unwrap_err(),
        DesktopError::RequestExpired,
        "an inode replacement cannot reuse the old content proof"
    );
}

#[test]
fn chmod_expires_a_proofed_reference_even_when_content_is_unchanged() {
    let fixture = Fixture::new();
    let path = fixture.private_file("chmod-proofed.bin", b"permission fixture bytes");
    let reference = NativeReference::selected_file(path.clone())
        .expect("small synthetic file is selected before chmod");
    let before = metadata_tuple(&path);
    let new_mode = if before.7 & 0o200 == 0 { 0o640 } else { 0o400 };
    fs::set_permissions(&path, fs::Permissions::from_mode(new_mode))
        .expect("synthetic file mode changes");
    assert_ne!(metadata_tuple(&path).7, before.7, "chmod changes the mode");
    assert_eq!(
        reference.validate().unwrap_err(),
        DesktopError::RequestExpired,
        "a mode change invalidates the content proof"
    );
}

#[test]
fn old_serialized_identity_ignores_future_identity_fields_and_stays_strict() {
    let fixture = Fixture::new();
    let path = fixture.private_file("legacy-identity.bin", b"legacy synthetic bytes");
    let selected =
        NativeReference::selected_file(path.clone()).expect("small synthetic file is selected");
    let mut encoded = remove_content_proof(
        serde_json::to_value(&selected).expect("selected synthetic file serializes"),
    );
    encoded["identity"]["future_identity_field"] = json!({"ignored": true});
    let legacy: NativeReference = serde_json::from_value(encoded)
        .expect("old identity payload ignores a newer nested identity field");
    legacy
        .validate()
        .expect("legacy identity validates before its file changes");

    let before = metadata_tuple(&path);
    fs::write(&path, b"update synthetic bytes")
        .expect("legacy fixture is rewritten at the same size");
    restore_mtime(&path, before.3, before.4);
    assert_eq!(
        legacy.validate().unwrap_err(),
        DesktopError::RequestExpired,
        "legacy references without a proof retain strict identity checks"
    );
}

#[test]
fn legacy_encrypted_reference_round_trip_keeps_identity_compatibility() {
    let fixture = Fixture::new();
    let path = fixture.private_file("legacy-encrypted.bin", b"legacy encrypted bytes");
    let selected = NativeReference::selected_file(path).expect("small synthetic file is selected");
    let legacy = remove_content_proof(
        serde_json::to_value(&selected).expect("selected synthetic file serializes"),
    );
    let plaintext = serde_json::to_vec(&json!({"references":[legacy]}))
        .expect("legacy reference body serializes");
    let crypto = crate::crypto::SecretBox::from_key_material(&"11".repeat(32)).unwrap();
    let (ciphertext, nonce) = crypto
        .encrypt(&plaintext)
        .expect("legacy reference body encrypts");
    let recovered = crypto
        .decrypt(&ciphertext, &nonce)
        .expect("legacy reference body decrypts");
    let body: Value =
        serde_json::from_slice(&recovered).expect("legacy encrypted reference body deserializes");
    let references: Vec<NativeReference> = serde_json::from_value(body["references"].clone())
        .expect("legacy encrypted references deserialize");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].kind(), "file");
    references[0]
        .validate()
        .expect("legacy encrypted identity remains valid before mutation");
}
