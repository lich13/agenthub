use super::{project, project_path, GrantValue, Grants, GRANT_AGE, MAX_GRANTS};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn fixture_reference() -> GrantValue {
    GrantValue::Reference(
        agenthub_core::codex::desktop::references::NativeReference::app(
            "fixture-app".into(),
            "app://fixture-app".into(),
        )
        .unwrap(),
    )
}

fn fixture_project(index: usize) -> GrantValue {
    GrantValue::Project {
        path: PathBuf::from(format!("/fixture/project-{index}")),
        device: 7,
        inode: index as u64 + 1,
    }
}

#[test]
fn opaque_reference_grants_are_revision_and_thread_scoped() {
    let mut grants = Grants::default();
    let value = fixture_reference();
    let first = grants.insert(12, Some("thread-a".into()), value.clone());

    assert!(grants.resolve(12, Some("thread-a"), &first).unwrap() == value);
    assert_eq!(
        grants.insert(12, Some("thread-a".into()), value.clone()),
        first
    );

    let other_thread = grants.insert(12, Some("thread-b".into()), value.clone());
    assert_ne!(other_thread, first);
    assert!(grants.resolve(12, Some("thread-b"), &first).is_err());
    assert!(grants.resolve(13, Some("thread-a"), &first).is_err());

    let next_revision = grants.insert(13, Some("thread-a".into()), value.clone());
    assert!(grants.resolve(12, Some("thread-a"), &first).is_err());
    assert!(grants.resolve(12, Some("thread-b"), &other_thread).is_err());
    assert!(
        grants
            .resolve(13, Some("thread-a"), &next_revision)
            .unwrap()
            == value
    );
}

#[test]
fn expired_grants_cannot_be_resolved_and_are_pruned_before_reissue() {
    let mut grants = Grants::default();
    let value = fixture_reference();
    let expired = grants.insert(4, Some("thread".into()), value.clone());
    grants.entries.get_mut(&expired).unwrap().created =
        Instant::now() - GRANT_AGE - Duration::from_secs(1);

    assert!(grants.resolve(4, Some("thread"), &expired).is_err());

    let renewed = grants.insert(4, Some("thread".into()), value.clone());
    assert_ne!(renewed, expired);
    assert_eq!(grants.entries.len(), 1);
    assert!(grants.resolve(4, Some("thread"), &renewed).unwrap() == value);
}

#[test]
fn grants_evict_the_oldest_entry_at_capacity() {
    let mut grants = Grants::default();
    let oldest = grants.insert(8, None, fixture_project(0));
    grants.entries.get_mut(&oldest).unwrap().created = Instant::now() - Duration::from_secs(1);

    let newest = (1..=MAX_GRANTS)
        .map(|index| grants.insert(8, None, fixture_project(index)))
        .last()
        .unwrap();

    assert_eq!(grants.entries.len(), MAX_GRANTS);
    assert!(grants.resolve(8, None, &oldest).is_err());
    assert!(grants.resolve(8, None, &newest).is_ok());
}

#[test]
fn selected_symlink_is_canonicalized_to_its_directory() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("selected-link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();

    let value = project(link).unwrap();
    let expected = target.canonicalize().unwrap();
    let returned = project_path(value).unwrap();
    assert!(
        returned == expected,
        "selection should retain the canonical target"
    );
}

#[test]
fn project_grant_rejects_replacement_of_the_selected_directory() {
    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("selected");
    let replacement = root.path().join("replacement");
    let moved = root.path().join("moved");
    fs::create_dir(&selected).unwrap();
    fs::create_dir(&replacement).unwrap();
    let grant = project(selected.clone()).unwrap();

    fs::rename(&selected, &moved).unwrap();
    fs::rename(&replacement, &selected).unwrap();

    assert!(
        project_path(grant).is_err(),
        "a grant must fail after its directory identity changes"
    );
}

#[test]
fn project_grant_rejects_replacement_with_a_symlink() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("selected");
    let target = root.path().join("target");
    fs::create_dir(&selected).unwrap();
    fs::create_dir(&target).unwrap();
    let grant = project(selected.clone()).unwrap();

    fs::remove_dir(&selected).unwrap();
    symlink(&target, &selected).unwrap();

    assert!(
        project_path(grant).is_err(),
        "a grant must reject a symlink substituted for its directory"
    );
}
