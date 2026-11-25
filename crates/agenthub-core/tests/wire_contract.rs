//! Test-only source contracts. No runtime reflection or wire-shape changes.
use quote::ToTokens;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use syn::{Attribute, Fields, Item};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn attrs(values: &[Attribute], names: &[&str]) -> Vec<String> {
    values
        .iter()
        .filter(|a| names.iter().any(|name| a.path().is_ident(name)))
        .map(|a| a.meta.to_token_stream().to_string())
        .collect()
}
fn fields(value: &Fields) -> Value {
    Value::Array(value.iter().enumerate().map(|(i, f)| json!({
        "name": f.ident.as_ref().map(ToString::to_string).unwrap_or_else(|| i.to_string()),
        "type": f.ty.to_token_stream().to_string(),
        "serde": attrs(&f.attrs, &["serde"])
    })).collect())
}
fn types(source: &str) -> BTreeMap<String, Value> {
    let mut found = BTreeMap::new();
    for item in syn::parse_file(source).unwrap().items {
        match item {
            Item::Struct(s)
                if attrs(&s.attrs, &["derive"])
                    .iter()
                    .any(|s| s.contains("Serialize") || s.contains("Deserialize")) =>
            {
                found.insert(s.ident.to_string(), json!({"kind":"struct", "serde":attrs(&s.attrs,&["serde"]), "fields":fields(&s.fields)}));
            }
            Item::Enum(e)
                if attrs(&e.attrs, &["derive"])
                    .iter()
                    .any(|s| s.contains("Serialize") || s.contains("Deserialize")) =>
            {
                found.insert(e.ident.to_string(), json!({"kind":"enum", "serde":attrs(&e.attrs,&["serde"]), "variants":e.variants.iter().map(|v| json!({"name":v.ident.to_string(),"serde":attrs(&v.attrs,&["serde"]),"fields":fields(&v.fields)})).collect::<Vec<_>>()}));
            }
            _ => {}
        }
    }
    found
}
fn source_files() -> Vec<PathBuf> {
    [
        "crates/agenthub-core/src",
        "crates/agenthub-webd/src/api",
        "crates/agenthub-ffi/src",
    ]
    .into_iter()
    .flat_map(|directory| walkdir::WalkDir::new(root().join(directory)))
    .map(Result::unwrap)
    .filter(|entry| entry.path().extension().is_some_and(|s| s == "rs"))
    .filter(|entry| {
        !entry.file_name().to_string_lossy().ends_with("tests.rs")
            && entry.file_name() != "test_support.rs"
    })
    .map(|entry| entry.into_path())
    .collect()
}
fn current_types() -> BTreeMap<String, BTreeMap<String, Value>> {
    source_files()
        .into_iter()
        .filter_map(|path| {
            let parsed = types(&std::fs::read_to_string(&path).unwrap());
            (!parsed.is_empty()).then(|| {
                (
                    path.strip_prefix(root())
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    parsed,
                )
            })
        })
        .collect()
}

fn fixture() -> Value {
    serde_json::from_slice(&std::fs::read(root().join("contracts/wire-types.json")).unwrap())
        .unwrap()
}

#[test]
fn cleanup_interaction_vectors_match_serialized_service_dtos() {
    use agenthub_core::{
        archive::HiddenThreadDeletePlan,
        services::cleanup::{
            UnifiedCleanupExecuteRequest, UnifiedCleanupPlan, UnifiedCleanupResult,
        },
    };
    fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(value: &Value) {
        let dto: T = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(dto).unwrap(), *value);
    }
    let cases: Value = serde_json::from_str(include_str!(
        "../../../contracts/parity-vectors/cleanup.json"
    ))
    .unwrap();
    round_trip::<UnifiedCleanupPlan>(&cases["preview"]);
    round_trip::<HiddenThreadDeletePlan>(&cases["hiddenStart"]);
    round_trip::<HiddenThreadDeletePlan>(&cases["hiddenPage"]);
    round_trip::<UnifiedCleanupExecuteRequest>(&cases["execute"]);
    round_trip::<UnifiedCleanupResult>(&cases["result"]);
}
#[test]
fn actual_rust_types_match_reviewed_wire_fields_and_serde_rules() {
    let actual = serde_json::to_value(current_types()).unwrap();
    let path = root().join("contracts/wire-types.json");
    let mut expected = fixture();
    if std::env::var_os("AGENTHUB_UPDATE_WIRE_FIXTURE").is_some() {
        expected["rust"] = actual.clone();
        if let Some(obj) = expected.as_object_mut() {
            obj.remove("rustAdapters");
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(&expected).unwrap() + "\n",
        )
        .unwrap();
    }
    assert_eq!(
        expected["rust"], actual,
        "wire field, optional/null, enum or serde attribute changed; follow Shared changes"
    );
}
#[test]
fn missing_fields_and_changed_serialization_are_detected() {
    let full=types("#[derive(Serialize)] #[serde(rename_all=\"camelCase\")] struct Example { #[serde(skip_serializing_if=\"Option::is_none\")] maybe: Option<String>, count: u64 }");
    for changed in [
        "#[derive(Serialize)] #[serde(rename_all=\"camelCase\")] struct Example { count: u64 }",
        "#[derive(Serialize)] #[serde(rename_all=\"snake_case\")] struct Example { maybe: Option<String>, count: u64 }",
        "#[derive(Serialize)] #[serde(rename_all=\"camelCase\")] struct Example { maybe: String, count: u64 }",
    ] { assert_ne!(full,types(changed)); }
}
#[test]
fn real_serialization_preserves_omission_null_and_enum_spelling() {
    use agenthub_core::{
        services::remote::{MachineTarget, RemoteConnectionView},
        session_storage::{SessionStorageSize, StorageScope, StorageStatus},
        user_message::{UserContextBlock, UserContextKind, UserMessage},
    };
    let storage = SessionStorageSize {
        bytes: None,
        scope: StorageScope::File,
        status: StorageStatus::Pending,
    };
    assert_eq!(
        serde_json::to_value(storage).unwrap(),
        json!({"bytes":null,"scope":"file","status":"pending"})
    );
    let connection = RemoteConnectionView {
        target: MachineTarget::Local,
        revision: 7,
        ssh_host: None,
        configured: false,
        active_machine_id: None,
        active_machine_name: None,
        machines: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(connection).unwrap(),
        json!({"target":"local","revision":7,"sshHost":null,"configured":false})
    );
    let mut message = UserMessage::default();
    message.id = "fixture".into();
    message.text = "request".into();
    let mut value = serde_json::to_value(message).unwrap();
    assert!(value.get("contextBlocks").is_none());
    value["contextBlocks"] = serde_json::to_value(vec![UserContextBlock {
        kind: UserContextKind::Environment,
        text: "fixture".into(),
    }])
    .unwrap();
    let roundtrip: UserMessage = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(roundtrip).unwrap(), value);
}

#[test]
fn registry_maps_to_real_types() {
    let f = fixture();
    let registry: Value = serde_json::from_slice(
        &std::fs::read(root().join("contracts/agenthub-contract.json")).unwrap(),
    )
    .unwrap();
    let mappings = f["catalog"].as_object().unwrap();
    assert_eq!(
        mappings.keys().collect::<Vec<_>>(),
        registry["dtoCatalog"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>()
    );
    let actual = current_types();
    for (name, mapping) in mappings {
        if let Some(reference) = mapping["rust"].as_str() {
            let (file, ty) = reference.split_once('#').unwrap();
            assert!(
                actual.get(file).is_some_and(|m| m.contains_key(ty)),
                "{name}: missing actual Rust type {reference}"
            );
        } else {
            assert!(
                mapping["freeJson"]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty()),
                "{name}: unclassified dynamic JSON"
            );
        }
    }
}
