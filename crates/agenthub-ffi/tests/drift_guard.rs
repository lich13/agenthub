use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use syn::{visit::Visit, Expr, ItemFn, Lit, Pat};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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
