use super::*;
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const SMALL_PNG: &[u8] = b"\x89PNG\r\n\x1a\nsynthetic image bytes";

struct Fixture {
    base: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "agenthub-desktop-attachments-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&base).expect("synthetic attachment fixture directory is created");
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700))
            .expect("synthetic attachment fixture directory is private");
        Self {
            root: base.join("store"),
            base,
        }
    }

    fn open(&self, now: u64) -> AttachmentStore {
        AttachmentStore::open(
            self.root.clone(),
            SecretBox::from_key_material(&"11".repeat(32)).unwrap(),
            now,
        )
        .expect("private synthetic attachment store opens")
    }

    fn path(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .expect("synthetic attachment permissions are set");
}

fn write_private_fixture_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .expect("synthetic private file is created");
    file.write_all(bytes)
        .expect("synthetic private file contents are written");
    file.sync_all().expect("synthetic private file is synced");
}

fn reference_path(reference: &NativeReference) -> &Path {
    let NativeReference::File { path, .. } = reference else {
        panic!("synthetic attachment reference is a file");
    };
    path
}

fn json_keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("serialized reference object exists")
        .keys()
        .cloned()
        .collect()
}

fn test_key() -> SecretBox {
    SecretBox::from_key_material(&"11".repeat(32)).unwrap()
}

fn wrong_test_key() -> SecretBox {
    SecretBox::from_key_material(&SecretBox::random_key_material())
        .expect("synthetic wrong key is valid key material")
}

fn sparse_png(store: &AttachmentStore, ordinal: usize, size: u64) -> NativeReference {
    let path = store.root.join(format!("{ordinal:064x}.png"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .expect("synthetic sparse PNG is created");
    file.write_all(PNG_SIGNATURE)
        .expect("synthetic sparse PNG signature is written");
    file.set_len(size)
        .expect("synthetic sparse PNG length is set");
    file.sync_all().expect("synthetic sparse PNG is synced");
    drop(file);
    NativeReference::selected_file(path).expect("synthetic sparse PNG reference is valid")
}

#[test]
fn imports_use_private_random_names_and_keep_the_existing_file_reference_shape() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let first = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic PNG is imported");
    let second = store
        .import_png(SMALL_PNG, 100)
        .expect("second synthetic PNG is imported");
    let first_path = reference_path(&first);
    let second_path = reference_path(&second);

    let root_metadata =
        fs::symlink_metadata(&store.root).expect("private attachment root metadata exists");
    assert!(root_metadata.is_dir());
    assert_eq!(root_metadata.mode() & 0o7777, 0o700);
    assert_eq!(root_metadata.uid(), unsafe { libc::geteuid() });

    assert_ne!(first_path, second_path, "imported names are randomized");
    for (reference, path) in [(&first, first_path), (&second, second_path)] {
        assert_eq!(path.parent(), Some(store.root.as_path()));
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("synthetic attachment file name is UTF-8");
        let stem = name.strip_suffix(".png").expect("PNG suffix is present");
        assert_eq!(stem.len(), 64);
        assert!(stem.bytes().all(|byte| byte.is_ascii_hexdigit()));

        let metadata = fs::symlink_metadata(path).expect("imported image metadata exists");
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o7777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.len(), SMALL_PNG.len() as u64);
        assert_eq!(reference.kind(), "image");

        let encoded =
            serde_json::to_value(reference).expect("legacy native file reference serializes");
        assert_eq!(encoded["kind"], "file");
        assert_eq!(encoded["name"], "粘贴图片.png");
        assert_eq!(encoded["image"], true);
        assert_eq!(
            json_keys(&encoded),
            BTreeSet::from([
                "image".to_owned(),
                "identity".to_owned(),
                "kind".to_owned(),
                "name".to_owned(),
                "path".to_owned(),
            ]),
            "private image import keeps the existing NativeReference::File fields"
        );
        assert_eq!(
            json_keys(&encoded["identity"]),
            BTreeSet::from([
                "changed".to_owned(),
                "changed_ns".to_owned(),
                "content_proof".to_owned(),
                "device".to_owned(),
                "inode".to_owned(),
                "modified".to_owned(),
                "modified_ns".to_owned(),
                "size".to_owned(),
            ])
        );
    }
}

#[test]
fn import_accepts_exactly_twenty_mib_and_rejects_invalid_or_oversized_pngs() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);

    assert_eq!(
        store.import_png(b"not a PNG", 100).err(),
        Some("粘贴图片格式无效")
    );
    let mut exact_limit = vec![b'x'; MAX_IMAGE_BYTES];
    exact_limit[..PNG_SIGNATURE.len()].copy_from_slice(PNG_SIGNATURE);
    let accepted = store
        .import_png(&exact_limit, 100)
        .expect("PNG at the 20 MiB boundary is accepted");
    assert_eq!(
        fs::metadata(reference_path(&accepted))
            .expect("boundary image metadata exists")
            .len(),
        MAX_IMAGE_BYTES as u64
    );

    let mut over_limit = vec![b'x'; MAX_IMAGE_BYTES + 1];
    over_limit[..PNG_SIGNATURE.len()].copy_from_slice(PNG_SIGNATURE);
    assert_eq!(
        store.import_png(&over_limit, 100).err(),
        Some("粘贴图片超过 20MiB")
    );
    assert_eq!(store.index.entries.len(), 1);
    assert_eq!(
        fs::read_dir(&store.root)
            .expect("attachment root can be listed")
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|value| value == "png"))
            .count(),
        1,
        "invalid and oversized imports do not leave unindexed files"
    );
}

#[test]
fn import_stops_at_the_256_image_limit() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);

    for _ in 0..MAX_IMAGES {
        store
            .import_png(SMALL_PNG, 100)
            .expect("PNG within the image-count boundary is accepted");
    }
    assert_eq!(store.index.entries.len(), MAX_IMAGES);
    assert_eq!(
        store.import_png(SMALL_PNG, 100).err(),
        Some("粘贴附件存储已满，请使用文件选择器添加图片")
    );
    assert_eq!(store.index.entries.len(), MAX_IMAGES);
}

#[test]
fn retained_files_survive_queue_failure_release_expiry_and_reopen() {
    let fixture = Fixture::new();
    let mut store = fixture.open(10_000);
    let reference = store
        .import_png(SMALL_PNG, 10_000)
        .expect("synthetic draft is imported");
    let path = reference_path(&reference).to_path_buf();
    let root = store.root.clone();

    store
        .retain_before_send(std::slice::from_ref(&reference))
        .expect("durable retain marker is written before queue handoff");
    assert!(store.index.entries[0].state == Retention::Retained);
    // Simulate a queue failure by not handing the retained reference to a queue.
    drop(store);

    let late = 10_000 + DRAFT_SECONDS + 24 * 60 * 60;
    let mut reopened = AttachmentStore::open(root.clone(), test_key(), late)
        .expect("retained attachment store reopens after draft expiry");
    assert!(reopened.index.entries[0].state == Retention::Retained);
    assert!(
        path.exists(),
        "retained image survives a failed queue handoff"
    );

    reopened
        .release(std::slice::from_ref(&reference))
        .expect("draft release leaves retained image alone");
    reopened
        .expire(late + DRAFT_SECONDS * 2)
        .expect("draft expiration leaves retained image alone");
    assert!(path.exists());
    assert!(reopened.index.entries[0].state == Retention::Retained);
    drop(reopened);

    let final_open = AttachmentStore::open(root, test_key(), late + DRAFT_SECONDS * 3)
        .expect("retained attachment store survives another reopen");
    assert!(path.exists());
    assert!(final_open.index.entries[0].state == Retention::Retained);
}

#[test]
fn release_and_thirty_minute_expiry_remove_only_drafts_at_the_boundary() {
    let fixture = Fixture::new();
    let mut store = fixture.open(50_000);
    let manually_released = store
        .import_png(SMALL_PNG, 50_000)
        .expect("synthetic draft for explicit release is imported");
    let expiring = store
        .import_png(SMALL_PNG, 50_000)
        .expect("synthetic draft for expiration is imported");
    let released_path = reference_path(&manually_released).to_path_buf();
    let expiring_path = reference_path(&expiring).to_path_buf();

    store
        .release(std::slice::from_ref(&manually_released))
        .expect("explicit draft release succeeds");
    assert!(!released_path.exists(), "released draft is removed");
    assert!(
        expiring_path.exists(),
        "unreleased draft remains before expiry"
    );
    store
        .expire(50_000 + DRAFT_SECONDS - 1)
        .expect("draft one second before expiry is retained");
    assert!(expiring_path.exists());
    store
        .expire(50_000 + DRAFT_SECONDS)
        .expect("draft expires at thirty minutes");
    assert!(!expiring_path.exists(), "expired draft is removed");
    assert!(store.index.entries.is_empty());
}

#[test]
fn corrupted_and_missing_indexes_preserve_attachment_files_and_fail_closed() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before index corruption");
    let root = store.root.clone();
    let image = reference_path(&reference).to_path_buf();
    drop(store);

    fs::write(root.join(INDEX), b"damaged encrypted index")
        .expect("synthetic encrypted index is corrupted");
    assert!(AttachmentStore::open(root.clone(), test_key(), 100).is_err());
    assert!(
        image.exists(),
        "corrupt ownership metadata never deletes the image"
    );

    fs::remove_file(root.join(INDEX)).expect("synthetic ownership index is removed");
    assert!(AttachmentStore::open(root.clone(), test_key(), 100).is_err());
    assert!(
        image.exists(),
        "missing ownership metadata leaves the image untouched"
    );
    assert!(
        !root.join(INDEX).exists(),
        "failed recovery does not invent ownership"
    );
}

#[test]
fn wrong_key_and_unknown_orphan_fail_closed_without_deleting_files() {
    let wrong_key_fixture = Fixture::new();
    let mut store = wrong_key_fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before wrong-key check");
    let root = store.root.clone();
    let image = reference_path(&reference).to_path_buf();
    drop(store);
    assert!(AttachmentStore::open(root.clone(), wrong_test_key(), 100).is_err());
    assert!(image.exists());
    assert!(root.join(INDEX).exists());

    let orphan_fixture = Fixture::new();
    let mut store = orphan_fixture.open(100);
    let root = store.root.clone();
    store
        .persist(store.index.clone())
        .expect("synthetic empty ownership index is persisted");
    let orphan = root.join(format!("{}.png", "ab".repeat(32)));
    write_private_fixture_file(&orphan, SMALL_PNG);
    drop(store);

    assert!(AttachmentStore::open(root, test_key(), 100).is_err());
    assert!(
        orphan.exists(),
        "unknown orphan is preserved for safe recovery"
    );
}

#[test]
fn symlink_replacement_is_never_unlinked_as_an_owned_image() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before symlink replacement");
    let path = reference_path(&reference).to_path_buf();
    let target = fixture.path("symlink-target.png");
    write_private_fixture_file(&target, b"external synthetic target");
    fs::remove_file(&path).expect("synthetic imported image is removed for replacement");
    symlink(&target, &path).expect("attachment path is replaced by a symlink");

    assert!(store.release(std::slice::from_ref(&reference)).is_err());
    assert!(fs::symlink_metadata(&path)
        .expect("replacement symlink remains")
        .file_type()
        .is_symlink());
    assert_eq!(
        fs::read(&target).expect("symlink target remains readable"),
        b"external synthetic target"
    );
}

#[test]
fn hardlink_replacement_is_never_unlinked_from_the_external_target() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before hardlink replacement");
    let path = reference_path(&reference).to_path_buf();
    let external = fixture.path("hardlink-target.png");
    fs::hard_link(&path, &external).expect("synthetic external hardlink is created");

    assert!(store.release(std::slice::from_ref(&reference)).is_err());
    assert!(path.exists(), "hardlinked attachment stays in the store");
    assert!(external.exists(), "hardlink target is not unlinked");
    assert_eq!(
        fs::read(&external).expect("hardlink target remains readable"),
        SMALL_PNG
    );
}

#[test]
fn replaced_store_root_identity_prevents_deletion_from_the_moved_root() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before root replacement");
    let root = store.root.clone();
    let moved_root = root
        .parent()
        .expect("synthetic store has a fixture parent")
        .join("moved-store");
    let moved_image = moved_root.join(
        reference_path(&reference)
            .file_name()
            .expect("synthetic attachment file name exists"),
    );
    fs::rename(&root, &moved_root).expect("synthetic private store root is moved");
    fs::create_dir(&root).expect("replacement root directory is created");
    set_mode(&root, 0o700);

    assert!(store.release(std::slice::from_ref(&reference)).is_err());
    assert!(
        moved_image.exists(),
        "replacement root cannot delete moved image data"
    );
    assert!(
        moved_root.join(INDEX).exists(),
        "moved ownership index is preserved"
    );
}

#[test]
fn changed_file_identity_prevents_deletion_of_a_replacement_target() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let reference = store
        .import_png(SMALL_PNG, 100)
        .expect("synthetic attachment is imported before identity replacement");
    let path = reference_path(&reference).to_path_buf();
    let replacement = b"different synthetic replacement file with a different identity";
    fs::remove_file(&path).expect("synthetic original image is removed for replacement");
    write_private_fixture_file(&path, replacement);

    assert!(store.release(std::slice::from_ref(&reference)).is_err());
    assert_eq!(
        fs::read(&path).expect("replacement target remains readable"),
        replacement,
        "stale file identity cannot delete replacement data"
    );
}

#[test]
fn released_crash_intermediate_states_are_recovered_on_reopen() {
    for unlink_before_reopen in [false, true] {
        let fixture = Fixture::new();
        let mut store = fixture.open(100);
        let reference = store
            .import_png(SMALL_PNG, 100)
            .expect("synthetic draft for crash recovery is imported");
        let root = store.root.clone();
        let image = reference_path(&reference).to_path_buf();
        let mut released = store.index.clone();
        released.entries[0].state = Retention::Released;
        store
            .persist(released)
            .expect("synthetic released marker is durably persisted");
        if unlink_before_reopen {
            fs::remove_file(&image).expect("synthetic unlink-before-index-cleanup is applied");
        }
        drop(store);

        let recovered = AttachmentStore::open(root, test_key(), 100)
            .expect("released crash intermediate state is recovered");
        assert!(!image.exists());
        assert!(recovered.index.entries.is_empty());
    }
}

#[test]
fn sparse_private_fixture_reaches_the_exact_512_mib_store_limit() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let full_images = 25usize;
    let remainder = MAX_STORE_BYTES - 8 - (full_images as u64 * MAX_IMAGE_BYTES as u64);
    assert!(remainder > 0 && remainder <= MAX_IMAGE_BYTES as u64);

    for ordinal in 1..=full_images {
        let reference = sparse_png(&store, ordinal, MAX_IMAGE_BYTES as u64);
        store.index.entries.push(Entry {
            reference,
            created: 100,
            state: Retention::Retained,
        });
    }
    let reference = sparse_png(&store, full_images + 1, remainder);
    store.index.entries.push(Entry {
        reference,
        created: 100,
        state: Retention::Retained,
    });
    let index = store.index.clone();
    store
        .persist(index)
        .expect("synthetic sparse ownership index is persisted");
    assert_eq!(
        store
            .validate_inventory()
            .expect("sparse inventory is valid"),
        MAX_STORE_BYTES - 8
    );

    let exact_boundary = store
        .import_png(PNG_SIGNATURE, 100)
        .expect("eight bytes fit exactly at the 512 MiB boundary");
    assert_eq!(
        store
            .validate_inventory()
            .expect("exact-cap inventory is valid"),
        MAX_STORE_BYTES
    );
    let entry_count = store.index.entries.len();
    assert_eq!(
        store.import_png(PNG_SIGNATURE, 100).err(),
        Some("粘贴附件存储已满，请使用文件选择器添加图片")
    );
    assert_eq!(store.index.entries.len(), entry_count);
    assert!(reference_path(&exact_boundary).exists());
}

#[cfg(target_os = "macos")]
struct DenyCreateAcl(PathBuf);

#[cfg(target_os = "macos")]
impl DenyCreateAcl {
    fn new(path: &Path) -> Self {
        use std::process::{Command, Stdio};

        let status = Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone deny add_file,delete_child")
            .arg(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("macOS ACL tool is available for the synthetic failure fixture");
        assert!(status.success(), "synthetic ACL blocks new files");
        Self(path.to_path_buf())
    }

    fn clear(&self) {
        use std::process::{Command, Stdio};

        let status = Command::new("/bin/chmod")
            .arg("-N")
            .arg(&self.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("macOS ACL tool remains available for fixture cleanup");
        assert!(status.success(), "synthetic ACL is removed");
    }
}

#[cfg(target_os = "macos")]
impl Drop for DenyCreateAcl {
    fn drop(&mut self) {
        use std::process::{Command, Stdio};

        let _ = Command::new("/bin/chmod")
            .arg("-N")
            .arg(&self.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn atomic_index_write_failure_faults_the_store_and_blocks_later_mutations() {
    let fixture = Fixture::new();
    let mut store = fixture.open(100);
    let root = store.root.clone();
    let acl = DenyCreateAcl::new(&root);
    assert_eq!(
        fs::metadata(&root)
            .expect("synthetic root metadata remains available")
            .mode()
            & 0o7777,
        0o700,
        "ACL leaves the private POSIX mode intact"
    );

    assert!(store.persist(store.index.clone()).is_err());
    assert!(
        store.faulted,
        "failed atomic index creation faults the store"
    );
    acl.clear();
    assert!(store.import_png(SMALL_PNG, 100).is_err());
    assert!(
        !root.join(INDEX).exists(),
        "failed atomic write leaves no partial index"
    );
    assert!(
        fs::read_dir(&root)
            .expect("failed-write fixture root can be listed")
            .next()
            .is_none(),
        "failed atomic write leaves no temporary files"
    );
}
