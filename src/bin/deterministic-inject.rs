use paperless_llm_workflows::extract::LLModelExtractor;
use paperless_llm_workflows::types::FieldExtract;
use schemars::{json_schema, schema_for};
use serde_json::{Value, json};
use std::path::Path;

fn schema_with_enum(enum_values: &[&str]) -> schemars::Schema {
    let enum_list: Vec<Value> = enum_values.iter().map(|s| json!(s)).collect();
    let value_schema = json_schema!({
        "type": "string",
        "enum": enum_list
    });
    let mut base_schema = schema_for!(FieldExtract);
    if let Some(properties) = base_schema.get_mut("properties") {
        if let Some(description_schema) = properties.get_mut("description") {
            *description_schema = json_schema!({ "const": "Correspondent" })
                .as_value()
                .clone();
        }
        if let Some(format_schema) = properties.get_mut("format") {
            *format_schema = json_schema!({
                "type": "object",
                "properties": {
                    "one_of": { "const": enum_list }
                },
                "required": ["one_of"]
            })
            .as_value()
            .clone();
        }
        if let Some(prop) = properties.as_object_mut() {
            let key_name = "most_likely_value_reasoning_summarized";
            prop.shift_insert(
                2,
                key_name.to_string(),
                json_schema!({ "type": "string" }).to_value(),
            );
            prop.get_mut("required")
                .map(|r| r.as_array_mut().map(|rv| rv.push(json!(key_name))));
        }
        if let Some(value_schema_entry) = properties.get_mut("value") {
            *value_schema_entry = value_schema.as_value().clone();
        }
    }
    base_schema
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model_path = args
        .get(1)
        .expect("Usage: deterministic-inject <model_path.gguf> [num_gpu_layers]");
    let num_gpu_layers: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(999);

    println!("Loading model from: {}", model_path);
    let mut extractor = LLModelExtractor::new(Path::new(model_path), num_gpu_layers, Some(4096))
        .expect("Failed to load model");

    let enum_values = vec![
        "City Water Department",
        "National Power Grid Authority",
        "Federal Tax Service",
        "Municipal Waste Management",
        "Regional Transit Commission",
        "County Health Department",
        "State Insurance Commissioner",
        "International Shipping Corp",
        "Global Telecom Services Inc",
        "Pacific Electric Utilities",
    ];
    let enum_refs: Vec<&str> = enum_values.iter().map(|s| *s).collect();
    let schema = schema_with_enum(&enum_refs);

    println!("\nSchema has {} enum variants:\n", enum_values.len());
    for (i, v) in enum_values.iter().enumerate() {
        println!("  [{}] {}", i, v);
    }

    let doc_content = "Your monthly electric bill from Pacific Electric Utilities is attached. \
         Please pay the amount of $142.50 by the due date.";
    let doc_data = json!({ "content": doc_content });

    println!("\nDocument content: \"{}\"", doc_content);
    println!("\nGenerating with grammar-constrained extraction...\n");

    let result = extractor
        .extract(&doc_data, &schema, true)
        .expect("Extraction failed");

    println!(
        "\nResult: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );
}
