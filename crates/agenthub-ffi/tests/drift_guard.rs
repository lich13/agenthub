use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use syn::{visit::Visit, Expr, ItemFn, Lit, Pat};

const DERIVED_TAURI_SOURCES: &[(&str, &str)] = &[
    (
        "src-tauri/src/ci_notifications.rs",
        "83caf475de3156995173f054a4ac9e8ae7beebe9028ce37022002149d8ea9f8c",
    ),
    (
        "src-tauri/src/codex_desktop.rs",
        "c11887e13f21598dd50bf0466029d8e5a255fd30ce81ff97bd962cbd5fd1d2e9",
    ),
    (
        "src-tauri/src/codex_desktop/attachments.rs",
        "32a6551dd088ef02e1edd74855c32dd6149ccd26f9048610b0d5f740671ae6e2",
    ),
    (
        "src-tauri/src/codex_desktop/clipboard.rs",
        "07b2d881544d550eceb0b7daf9df5393308f2b6daefb10c849a4e7e746469f2c",
    ),
    (
        "src-tauri/src/codex_desktop/clipboard_tests.rs",
        "4ca4a7ebbff2fa46156b16857e22d8e31d17a3884d1e7961a70938cd8a2c5efb",
    ),
    (
        "src-tauri/src/codex_desktop/selection.rs",
        "da7ecc5b202ec6505881f3a2069dd11dd1a7e7075f12b92b843531ad114f98f5",
    ),
    (
        "src-tauri/src/commands/claude.rs",
        "373d4e2dd96af6b5ff2e340272fb52a5c6fe86925a7a7d85479330c2066dbfb0",
    ),
    (
        "src-tauri/src/commands/grok.rs",
        "ea471795b315c2105689daecb35c87bbd34ece7f666cb3279db34e87b54927fa",
    ),
    (
        "src-tauri/src/commands/jobs.rs",
        "f32b31d6137e607d9f049180b5c81028254db694976cdf57d45f6b07615f3f59",
    ),
    (
        "src-tauri/src/commands/mod.rs",
        "2677480ccb7ce75abdd08cdca1a408b80bb899d312df7da4425dda22af9bf7f0",
    ),
    (
        "src-tauri/src/commands/plans.rs",
        "e1aef6bf7f1e437618884962ad29466fcbbc58e3d3fc80b8dddfdc1bdca0049a",
    ),
    (
        "src-tauri/src/commands/probe.rs",
        "d47b2e915988be9daed314073fc1684bef408e09dec65c896e23fb3a742ba5a2",
    ),
    (
        "src-tauri/src/commands/provider_sessions.rs",
        "4129453f0fc4661d6157a8bc45e2b41f964392fa990ebeea167d66fcf2493a1a",
    ),
    (
        "src-tauri/src/commands/sessions.rs",
        "4245589137b17796f301ee359016da64d99e01b8d04786703dee93828277e1dc",
    ),
    (
        "src-tauri/src/commands/settings.rs",
        "8b061aeecf753b9c3fe1538c401bced327995de7917c14d28f0c1411ec90c6fa",
    ),
    (
        "src-tauri/src/commands/system.rs",
        "ae7f77275f54330bfa2409cf6f7a8f11beb24e9f46ec5b267ec601b76850caa5",
    ),
    (
        "src-tauri/src/commands/threads.rs",
        "08c2a967f0e07311c25d5ac24570825db79636ace5f275f6c60dee2887d00f1c",
    ),
    (
        "src-tauri/src/commands/updates.rs",
        "17c047d124f5a3c3cd3f7382d5c212aa85d2731b440854150a602eced842d0c9",
    ),
    (
        "src-tauri/src/remote.rs",
        "18a17328e1f20de5410ce9d88b400d333318db3aa2f1df4553b7c49d4ce75e76",
    ),
    (
        "src-tauri/src/services/mod.rs",
        "00a613da857540b208b73ddfa0fd74d4b5e2163cdb0838069c3e8eb749a968a6",
    ),
    (
        "src-tauri/src/services/threads.rs",
        "85dadde2acd5419e41c04aab7dd7488019f4d79825ee8e4ec745134e34cf07ea",
    ),
    (
        "src-tauri/src/services/updates.rs",
        "d9f1e700236ed3c5e75afb32cd184c2c974491b681a39204d75d281ca88a89b8",
    ),
    (
        "src-tauri/tests/probe_commands.rs",
        "2c1883c116f6bec3dbb87cb58133392afcc373e3509dd5282fe9e63010e1bb15",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn derived_src_tauri_files_have_zero_unreviewed_sha256_drift() {
    let root = repo_root();
    if !root.join("src-tauri").is_dir() {
        return;
    }
    for (rel, expected_sha256) in DERIVED_TAURI_SOURCES {
        let bytes = std::fs::read(root.join(rel))
            .unwrap_or_else(|err| panic!("missing derived source {rel}: {err}"));
        let actual = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            &actual, expected_sha256,
            "src-tauri file {rel} changed without syncing crates/agenthub-ffi"
        );
    }
}

#[derive(Default)]
struct DispatchArmsVisitor {
    in_dispatch_fn: bool,
    arms: Vec<String>,
}

impl<'ast> Visit<'ast> for DispatchArmsVisitor {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        if node.sig.ident == "dispatch" {
            self.in_dispatch_fn = true;
            syn::visit::visit_item_fn(self, node);
            self.in_dispatch_fn = false;
        }
    }

    fn visit_expr_match(&mut self, node: &'ast syn::ExprMatch) {
        if self.in_dispatch_fn {
            if let Expr::Path(expr_path) = &*node.expr {
                if expr_path.path.is_ident("command") {
                    for arm in &node.arms {
                        collect_pat_strings(&arm.pat, &mut self.arms);
                    }
                    return;
                }
            }
        }
        syn::visit::visit_expr_match(self, node);
    }
}

fn collect_pat_strings(pat: &Pat, out: &mut Vec<String>) {
    match pat {
        Pat::Lit(pat_lit) => {
            if let Lit::Str(s) = &pat_lit.lit {
                out.push(s.value());
            }
        }
        Pat::Or(pat_or) => {
            for case in &pat_or.cases {
                collect_pat_strings(case, out);
            }
        }
        _ => {}
    }
}

#[test]
fn dispatch_ast_arms_match_all_73_contract_tauri_commands() {
    let root = repo_root();
    let dispatch_src =
        std::fs::read_to_string(root.join("crates/agenthub-ffi/src/commands/dispatch.rs")).unwrap();
    let ast = syn::parse_file(&dispatch_src).unwrap();
    let mut visitor = DispatchArmsVisitor::default();
    visitor.visit_file(&ast);

    let actual_arms: BTreeSet<String> = visitor.arms.iter().cloned().collect();
    assert_eq!(
        visitor.arms.len(),
        actual_arms.len(),
        "duplicate command arm in dispatch.rs"
    );

    let contract_raw =
        std::fs::read_to_string(root.join("contracts/agenthub-contract.json")).unwrap();
    let contract: serde_json::Value = serde_json::from_str(&contract_raw).unwrap();
    let expected_ids: BTreeSet<String> = contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| {
            action
                .get("tauriCommand")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| !s.is_empty())
        })
        .map(|action| action["id"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(expected_ids.len(), 73);
    assert_eq!(
        actual_arms, expected_ids,
        "dispatch.rs match arms drifted from contracts/agenthub-contract.json"
    );
}
