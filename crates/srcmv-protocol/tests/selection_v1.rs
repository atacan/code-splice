//! Semantic-selection v1 schema, golden-vector, and composition tests.

use std::fs;
use std::path::{Path, PathBuf};

use srcmv_protocol::parse_request;
use jsonschema::{Registry, Validator};
use serde_json::{Value, json};

const EDIT_REQUEST_SCHEMA_ID: &str = "https://codesplice.dev/schema/v1/request.schema.json";

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn selection_schema(name: &str) -> Value {
    read_json(
        &repository_root()
            .join("docs/schema/selection-v1")
            .join(name),
    )
}

fn selection_golden(name: &str) -> Value {
    read_json(
        &repository_root()
            .join("tests/golden/selection-v1")
            .join(name),
    )
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|error| panic!("{} must contain JSON: {error}", path.display()))
}

fn collect_json_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", directory.display()))
    {
        let path = entry.expect("directory entry must be readable").path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
}

fn collect_references<'a>(value: &'a Value, references: &mut Vec<&'a str>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_references(value, references);
            }
        }
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                references.push(reference);
            }
            for value in object.values() {
                collect_references(value, references);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn selection_validator(schema_name: &str) -> Validator {
    let request_schema = read_json(&repository_root().join("docs/schema/v1/request.schema.json"));
    let registry = Registry::new()
        .add(EDIT_REQUEST_SCHEMA_ID, request_schema)
        .expect("edit request schema ID must be a valid URI")
        .prepare()
        .expect("offline schema registry must prepare");
    jsonschema::draft202012::options()
        .with_registry(&registry)
        .build(&selection_schema(schema_name))
        .unwrap_or_else(|error| panic!("{schema_name} must compile: {error}"))
}

#[test]
fn every_selection_contract_json_file_should_parse() {
    let mut files = Vec::new();
    collect_json_files(
        &repository_root().join("docs/schema/selection-v1"),
        &mut files,
    );
    collect_json_files(
        &repository_root().join("tests/golden/selection-v1"),
        &mut files,
    );
    files.sort();

    assert!(
        !files.is_empty(),
        "selection contract must contain JSON files"
    );
    for path in files {
        read_json(&path);
    }
}

#[test]
fn selection_schema_internal_references_should_resolve() {
    for name in ["response.schema.json", "error.schema.json"] {
        let schema = selection_schema(name);
        let mut references = Vec::new();
        collect_references(&schema, &mut references);
        for reference in references
            .into_iter()
            .filter(|reference| reference.starts_with('#'))
        {
            let pointer = reference
                .strip_prefix('#')
                .expect("filtered reference must start with a fragment");
            assert!(
                schema.pointer(pointer).is_some(),
                "{name} contains unresolved reference {reference}"
            );
        }
    }
}

#[test]
fn selection_schemas_should_validate_against_the_draft_2020_12_metaschema() {
    for name in ["response.schema.json", "error.schema.json"] {
        let schema = selection_schema(name);
        jsonschema::draft202012::meta::validate(&schema).unwrap_or_else(|error| {
            panic!("{name} must satisfy the Draft 2020-12 metaschema: {error}")
        });
        selection_validator(name);
    }
}

#[test]
fn success_goldens_should_validate_against_the_compiled_offline_schema() {
    let validator = selection_validator("response.schema.json");

    for name in ["composition-selection.json", "success-position.json"] {
        let instance = selection_golden(name);
        validator
            .validate(&instance)
            .unwrap_or_else(|error| panic!("{name} must satisfy selection response v1: {error}"));
    }
}

#[test]
fn error_goldens_should_validate_against_the_compiled_offline_schema() {
    let validator = selection_validator("error.schema.json");

    for name in [
        "error-not-found.json",
        "error-ambiguous.json",
        "error-timeout.json",
    ] {
        let instance = selection_golden(name);
        validator
            .validate(&instance)
            .unwrap_or_else(|error| panic!("{name} must satisfy selection error v1: {error}"));
    }
}

#[test]
fn response_schema_should_reject_an_unknown_top_level_field() {
    let validator = selection_validator("response.schema.json");
    let mut instance = selection_golden("composition-selection.json");
    instance["unknown"] = json!(true);

    assert!(!validator.is_valid(&instance));
}

#[test]
fn response_schema_should_reject_a_missing_required_field() {
    let validator = selection_validator("response.schema.json");
    let mut instance = selection_golden("composition-selection.json");
    instance
        .as_object_mut()
        .expect("success golden must be an object")
        .remove("source");

    assert!(!validator.is_valid(&instance));
}

#[test]
fn response_schema_should_reject_an_unknown_position_encoding() {
    let validator = selection_validator("response.schema.json");
    let mut instance = selection_golden("composition-selection.json");
    instance["server"]["position_encoding"] = json!("utf-7");

    assert!(!validator.is_valid(&instance));
}

#[test]
fn success_schema_should_freeze_selection_v1_keys_and_limits() {
    let schema = selection_schema("response.schema.json");
    let required = json!([
        "selection_protocol_version",
        "workspace_identity_hash",
        "source",
        "query",
        "server",
        "matches",
        "warnings"
    ]);

    assert_eq!(schema["required"], required);
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["selection_protocol_version"]["const"],
        1
    );
    assert_eq!(schema["properties"]["matches"]["maxItems"], 1000);
    assert_eq!(schema["properties"]["warnings"]["maxItems"], 16);
    assert_eq!(schema["$defs"]["symbol_path"]["maxItems"], 256);
    assert_eq!(
        schema["$defs"]["warning"]["properties"]["code"]["const"],
        "OBSERVATION_MAY_BE_STALE"
    );
    assert_eq!(
        schema["$defs"]["match"]["properties"]["request_source"]["$ref"],
        "https://codesplice.dev/schema/v1/request.schema.json#/$defs/source"
    );
    let match_keys = schema["$defs"]["match"]["required"]
        .as_array()
        .expect("match required keys must be an array");
    assert!(match_keys.contains(&json!("lsp_range")));
    assert!(match_keys.contains(&json!("lsp_selection_range")));
}

#[test]
fn selection_error_schema_should_match_the_golden_registry() {
    let schema = selection_schema("error.schema.json");
    let registry = selection_golden("error-registry.json");

    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["selection_protocol_version"]["const"],
        1
    );
    assert_eq!(schema["x-codesplice-error-registry"], registry);
    let mut schema_codes = schema["properties"]["code"]["enum"]
        .as_array()
        .expect("schema error codes must be an array")
        .clone();
    schema_codes.sort_by_key(ToString::to_string);
    let mut registry_codes = registry
        .as_array()
        .expect("golden error registry must be an array")
        .iter()
        .map(|entry| entry["code"].clone())
        .collect::<Vec<_>>();
    registry_codes.sort_by_key(ToString::to_string);
    assert_eq!(schema_codes, registry_codes);
    let branches = schema["allOf"]
        .as_array()
        .expect("error mapping conditions must be an array");
    for mapping in registry
        .as_array()
        .expect("golden error registry must be an array")
    {
        let branch = branches
            .iter()
            .find(|branch| branch["if"]["properties"]["code"]["const"] == mapping["code"])
            .expect("every golden error must have a schema condition");
        assert_eq!(
            branch["then"]["properties"]["category"]["const"], mapping["category"],
            "{} category",
            mapping["code"]
        );
        assert_eq!(
            branch["then"]["properties"]["retryable"]["const"], mapping["retryable"],
            "{} retryability",
            mapping["code"]
        );
    }
    assert_eq!(
        schema["$defs"]["ambiguity_context"]["properties"]["candidates"]["maxItems"],
        50
    );
    assert_eq!(
        schema["$defs"]["candidate"]["properties"]["symbol_path"]["maxItems"],
        256
    );
}

#[test]
fn selection_error_examples_should_obey_the_golden_mapping() {
    let registry = selection_golden("error-registry.json");
    let entries = registry
        .as_array()
        .expect("golden error registry must be an array");

    for name in [
        "error-not-found.json",
        "error-ambiguous.json",
        "error-timeout.json",
    ] {
        let example = selection_golden(name);
        let mapping = entries
            .iter()
            .find(|entry| entry["code"] == example["code"])
            .unwrap_or_else(|| panic!("{name} code must occur in the golden registry"));
        assert_eq!(example["selection_protocol_version"], 1, "{name}");
        assert_eq!(example["category"], mapping["category"], "{name}");
        assert_eq!(example["retryable"], mapping["retryable"], "{name}");
    }
}

#[test]
fn success_examples_should_preserve_copy_ready_source_invariants() {
    for name in ["composition-selection.json", "success-position.json"] {
        let response = selection_golden(name);
        let source = &response["source"];
        let matches = response["matches"]
            .as_array()
            .expect("success matches must be an array");
        for selected_match in matches {
            let request_source = &selected_match["request_source"];
            assert_eq!(request_source["path"], source["path"], "{name}");
            assert_eq!(
                request_source["precondition"]["value"], source["sha256"],
                "{name}"
            );
            assert_eq!(
                request_source["selector"], selected_match["selector"],
                "{name}"
            );
            let start = selected_match["selector"]["start"]
                .as_u64()
                .expect("selector start must be an unsigned integer");
            let end = selected_match["selector"]["end"]
                .as_u64()
                .expect("selector end must be an unsigned integer");
            assert_eq!(
                selected_match["selected_byte_length"],
                end - start,
                "{name}"
            );
            assert!(
                end <= source["byte_length"]
                    .as_u64()
                    .expect("source byte length must be an unsigned integer"),
                "{name} selector must be inside the source snapshot"
            );
            assert_eq!(
                selected_match["symbol_path"]
                    .as_array()
                    .and_then(|path| path.last()),
                Some(&selected_match["name"]),
                "{name} breadcrumb must include the selected symbol"
            );
        }
        for warning in response["warnings"]
            .as_array()
            .expect("success warnings must be an array")
        {
            assert_eq!(warning["code"], "OBSERVATION_MAY_BE_STALE", "{name}");
        }
    }
}

#[test]
fn composition_request_source_should_copy_unchanged_into_protocol_v1() {
    let response = selection_golden("composition-selection.json");
    let standalone = selection_golden("composition-request-source.json");
    let request = selection_golden("composition-edit-request.json");

    assert_eq!(response["matches"][0]["request_source"], standalone);
    assert_eq!(request["operations"][0]["source"], standalone);
    let request_bytes =
        fs::read(repository_root().join("tests/golden/selection-v1/composition-edit-request.json"))
            .expect("composition request must be readable");
    let batch =
        parse_request(&request_bytes).expect("composed edit request must parse as protocol v1");
    assert_eq!(batch.operations.len(), 1);
}
