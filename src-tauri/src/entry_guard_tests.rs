#[cfg(test)]
mod tests {
    fn production_lib_source() -> &'static str {
        include_str!("lib.rs")
            .split("\n#[cfg(test)]")
            .next()
            .expect("lib source must include production section")
    }

    fn registered_invoke_command_paths() -> Vec<String> {
        let production_source = production_lib_source();
        let marker = "tauri::generate_handler![";
        let start = production_source
            .find(marker)
            .expect("lib source must include tauri generate_handler")
            + marker.len();
        let body = production_source[start..]
            .split_once(']')
            .map(|(commands, _)| commands)
            .expect("generate_handler block must close");
        // Inspect every registered path, including commands outside the older
        // commands/remote modules, so new host-only modules cannot escape this guard.
        body.split(',')
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn contract_actions() -> Vec<serde_json::Value> {
        let raw = include_str!("../../contracts/agenthub-contract.json");
        let value: serde_json::Value =
            serde_json::from_str(raw).expect("contract registry must be valid JSON");
        value
            .get("actions")
            .and_then(serde_json::Value::as_array)
            .expect("contract actions must be an array")
            .clone()
    }

    #[test]
    fn tauri_invoke_handler_matches_contract_registry() {
        let registered = registered_invoke_command_paths()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let expected = contract_actions()
            .into_iter()
            .filter_map(|action| {
                action
                    .get("tauriCommand")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            registered, expected,
            "Tauri invoke registration must be declared by contracts/agenthub-contract.json"
        );
    }
}
