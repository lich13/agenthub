use agenthub_ffi::commands::dispatch::REGISTERED_COMMAND_ENTRIES;
use std::collections::BTreeMap;

#[test]
fn executable_desktop_handlers_match_the_contract() {
    let contract: serde_json::Value =
        serde_json::from_str(include_str!("../../../contracts/agenthub-contract.json")).unwrap();
    let expected: BTreeMap<_, _> = contract["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| Some((a["id"].as_str()?, a["ffiHandler"].as_str()?)))
        .collect();
    let actual: BTreeMap<_, _> = REGISTERED_COMMAND_ENTRIES.iter().copied().collect();
    assert_eq!(actual.len(), REGISTERED_COMMAND_ENTRIES.len());
    assert_eq!(actual, expected);
}
