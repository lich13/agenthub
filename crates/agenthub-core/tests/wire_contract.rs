//! Test-only source contracts. No runtime reflection or wire-shape changes.
use quote::ToTokens;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use syn::{visit::Visit, Attribute, Fields, Item};

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
        "src-tauri/src",
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
fn desktop_contract_actions_roundtrip_and_send_rejects_client_paths() {
    use agenthub_core::services::codex_desktop::{
        DesktopActionRequest, DesktopCatalogRequest, DesktopChoice, DesktopThreadRequest,
        DesktopView,
    };
    for action in [
        json!({"type":"send","messageId":"fixture","text":"request","references":["opaque-reference"]}),
        json!({"type":"steer","messageId":"fixture","text":"request","references":[],"turnId":"turn-fixture"}),
        json!({"type":"stop","turnId":"turn-fixture"}),
        json!({"type":"cancelQueued","messageId":"fixture"}),
        json!({"type":"dismissUnknown","messageId":"fixture"}),
        json!({"type":"resumeQueue"}),
        json!({"type":"settings","model":"model-fixture","reasoningEffort":null,"mode":"plan"}),
        json!({"type":"decide","requestKey":"request-fixture","allow":false}),
        json!({"type":"answer","requestKey":"request-fixture","answers":[{"questionId":"question-fixture","answers":["answer"]}]}),
        json!({"type":"elicitation","requestKey":"request-fixture","accept":false,"content":null}),
    ] {
        let wire = json!({"revision":7,"sessionId":"session-fixture","action":action});
        let request: DesktopActionRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), wire);
        if matches!(wire["action"]["type"].as_str(), Some("send" | "steer")) {
            let mut with_path = wire;
            with_path["action"]["path"] = json!("/fixture/unselected");
            assert!(serde_json::from_value::<DesktopActionRequest>(with_path).is_err());
        }
    }
    let view = json!({
        "sessionId":"session-fixture","threadId":"thread-fixture","revision":7,
        "connected":false,"writable":false,"runtime":"idle","activeTurnId":null,
        "model":null,"reasoningEffort":null,"mode":null,"requests":[],"queue":[],"error":null
    });
    let decoded: DesktopView = serde_json::from_value(view.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), view);
    for confirm in [None, Some(false), Some(true)] {
        let mut wire = json!({"revision":7,"threadId":"thread-fixture"});
        if let Some(value) = confirm {
            wire["confirmOwner"] = json!(value);
        }
        let decoded: DesktopThreadRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.confirm_owner, confirm);
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    for force in [None, Some(false), Some(true)] {
        let mut wire = json!({"revision":7,"sessionId":"session-fixture","projectHandle":null,"kind":"modes","query":"","cursor":null});
        if let Some(value) = force {
            wire["forceRefetch"] = json!(value);
        }
        let decoded: DesktopCatalogRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(decoded.force_refetch, force);
        assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    }
    assert_eq!(
        serde_json::to_value(DesktopChoice { item: None }).unwrap(),
        json!({"item":null})
    );
}

#[derive(Default)]
struct RegisteredCommands(Vec<String>);
impl<'a> Visit<'a> for RegisteredCommands {
    fn visit_macro(&mut self, mac: &'a syn::Macro) {
        use syn::parse::Parser;
        if mac
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "generate_handler")
        {
            let paths = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated
                .parse2(mac.tokens.clone())
                .unwrap();
            self.0.extend(paths.iter().map(|p| {
                p.segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            }));
        }
    }
}
struct FunctionNames(Vec<String>);
impl<'a> Visit<'a> for FunctionNames {
    fn visit_item_fn(&mut self, f: &'a syn::ItemFn) {
        self.0.push(f.sig.ident.to_string());
    }
}
#[test]
fn registry_maps_to_real_types_and_registered_adapter_functions() {
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
    let mut registered = RegisteredCommands::default();
    registered.visit_file(
        &syn::parse_file(&std::fs::read_to_string(root().join("src-tauri/src/lib.rs")).unwrap())
            .unwrap(),
    );
    for action in registry["actions"].as_array().unwrap() {
        if let Some(adapter) = action["tauriCommand"].as_str() {
            let (module, function) = adapter.rsplit_once("::").unwrap();
            let path = format!("src-tauri/src/{}.rs", module.replace("::", "/"));
            let source =
                syn::parse_file(&std::fs::read_to_string(root().join(&path)).unwrap()).unwrap();
            let mut names = FunctionNames(vec![]);
            names.visit_file(&source);
            assert!(
                names.0.iter().any(|s| s == function),
                "missing actual Tauri function: {adapter}"
            );
            assert!(
                registered.0.iter().any(|name| name == adapter),
                "unregistered Tauri function: {adapter}"
            );
        }
    }
    use agenthub_core::services::commands::{RetiredSurface, RETIRED_COMMAND_REGISTRY};
    let listed = registry["retiredActions"].as_array().unwrap();
    let shared: std::collections::BTreeSet<_> = RETIRED_COMMAND_REGISTRY
        .iter()
        .filter(|r| r.surface == RetiredSurface::Shared)
        .map(|r| r.name)
        .collect();
    assert!(listed.iter().all(|v| shared.contains(v.as_str().unwrap())));
}
