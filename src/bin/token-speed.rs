use clap::Parser;
use paperless_llm_workflows::types::FieldExtract;
use paperless_llm_workflows::{LLModelExtractor, TokenGenerationStats};
use schemars::{json_schema, schema_for};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser, Debug)]
#[clap(name = "token-speed", about = "Token generation speed benchmark")]
struct Args {
    /// Path to a GGUF model file
    #[clap(short, long)]
    model: Option<PathBuf>,

    /// Directory containing GGUF model files to benchmark
    #[clap(long)]
    model_dir: Option<PathBuf>,

    /// Number of GPU layers for inference (0 = unlimited)
    #[clap(long, default_value = "999")]
    num_gpu_layers: usize,

    /// Maximum output tokens per generation run
    #[clap(long, default_value = "500")]
    max_tokens: usize,

    /// Number of runs to average per mode
    #[clap(long, default_value = "1")]
    runs: usize,

    /// Maximum context size (0 = model default)
    #[clap(long, default_value = "0")]
    max_ctx: usize,
}

fn grammar_schema() -> schemars::Schema {
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
    let enum_list: Vec<serde_json::Value> = enum_values.iter().map(|s| json!(s)).collect();

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

fn free_generate_prompt() -> String {
    "You are a helpful assistant. Write a detailed technical explanation about how transformer-based language models work, including attention mechanisms, positional encoding, and feed-forward networks. Explain the mathematical foundations and practical implications for natural language processing tasks."
        .to_string()
}

fn grammar_extract_prompt() -> serde_json::Value {
    json!({
        "content": "Your monthly electric bill from Pacific Electric Utilities is attached for the amount of $142.50. Please review the attached statement and pay by the due date indicated on the invoice. The account number is 4829103. For questions about your bill, contact our customer service department."
    })
}

fn find_gguf_models(dir: &Path) -> Vec<PathBuf> {
    let mut models = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.eq_ignore_ascii_case("gguf") {
                        models.push(path);
                    }
                }
            }
        }
    }
    models.sort();
    models
}

fn aggregate_stats(stats_list: &[TokenGenerationStats]) -> TokenGenerationStats {
    let mut agg = TokenGenerationStats::new();
    for s in stats_list {
        agg.add(s);
    }
    agg
}

fn run_grammar_benchmark(
    extractor: &mut LLModelExtractor,
    _max_tokens: usize,
    runs: usize,
) -> Vec<TokenGenerationStats> {
    let schema = grammar_schema();
    let prompt_data = grammar_extract_prompt();
    let mut results = Vec::with_capacity(runs);

    for i in 0..runs {
        eprint!("\r  Grammar run {}/{}", i + 1, runs);
        std::io::stdout().flush().ok();

        let (stats_tx, stats_rx) = std::sync::mpsc::sync_channel(64);
        let start = Instant::now();

        let handle = std::thread::spawn(move || {
            let mut last = TokenGenerationStats::new();
            for s in stats_rx.iter() {
                last = s;
            }
            last
        });

        let _ = extractor.extract(&prompt_data, &schema, false, Some(stats_tx));
        let stats = handle.join().unwrap();
        let elapsed = start.elapsed();

        let mut final_stats = stats;
        final_stats.sampled_elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
        results.push(final_stats);
    }
    eprintln!();
    results
}

fn run_free_benchmark(
    extractor: &mut LLModelExtractor,
    max_tokens: usize,
    runs: usize,
) -> Vec<TokenGenerationStats> {
    let prompt = free_generate_prompt();
    let mut results = Vec::with_capacity(runs);

    for i in 0..runs {
        eprint!("\r  Free-gen run {}/{}", i + 1, runs);
        std::io::stdout().flush().ok();

        let (stats_tx, stats_rx) = std::sync::mpsc::sync_channel(64);
        let start = Instant::now();

        let handle = std::thread::spawn(move || {
            let mut last = TokenGenerationStats::new();
            for s in stats_rx.iter() {
                last = s;
            }
            last
        });

        let _ = extractor.free_generate(&prompt, max_tokens, Some(stats_tx));
        let stats = handle.join().unwrap();
        let elapsed = start.elapsed();

        let mut final_stats = stats;
        final_stats.sampled_elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
        results.push(final_stats);
    }
    eprintln!();
    results
}

fn run_model_benchmark(
    model_path: &Path,
    num_gpu_layers: usize,
    max_ctx: usize,
    max_tokens: usize,
    runs: usize,
) {
    let model_name = model_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| model_path.display().to_string());

    eprintln!("Loading model: {}", model_name);
    let ctx_size = if max_ctx == 0 {
        None
    } else {
        Some(max_ctx as u32)
    };

    let mut extractor = match LLModelExtractor::new(model_path, num_gpu_layers, ctx_size) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("  Failed to load model: {}", e);
            return;
        }
    };
    eprintln!("Model loaded.\n");

    // Grammar-constrained benchmark
    eprintln!("Running grammar-constrained benchmark...");
    std::io::stdout().flush().ok();
    let grammar_stats = run_grammar_benchmark(&mut extractor, max_tokens, runs);
    let grammar_agg = aggregate_stats(&grammar_stats);

    // Free generation benchmark
    eprintln!("\nRunning free-generation benchmark...");
    std::io::stdout().flush().ok();
    let free_stats = run_free_benchmark(&mut extractor, max_tokens, runs);
    let free_agg = aggregate_stats(&free_stats);

    // Print results
    println!("\n{}", "=".repeat(80));
    println!("Model: {}", model_name);
    println!(
        "Config: max_tokens={}, runs={}, gpu_layers={}",
        max_tokens, runs, num_gpu_layers
    );
    println!("{}", "=".repeat(80));

    println_results(&grammar_agg, &free_agg, runs);
}

#[derive(tabled::Tabled)]
struct ResultRow {
    mode: String,
    prompt_tps: String,
    prompt_tokens: usize,
    inject_tps: String,
    inject_tokens: u64,
    inject_pct: String,
    sample_tps: String,
    sample_tokens: u64,
    total_tps: String,
    total_time: String,
    forward_passes: u64,
}

fn println_results(grammar: &TokenGenerationStats, free: &TokenGenerationStats, _runs: usize) {
    let rows = vec![
        ResultRow {
            mode: "grammar".to_string(),
            prompt_tps: format!("{:.1}", grammar.prompt_tps()),
            prompt_tokens: grammar.prompt_tokens,
            inject_tps: format!("{:.1}", grammar.injected_tps()),
            inject_tokens: grammar.injected_tokens,
            inject_pct: format!(
                "{:.1}%",
                if grammar.injected_tokens + grammar.sampled_tokens > 0 {
                    (grammar.injected_tokens as f64)
                        / (grammar.injected_tokens + grammar.sampled_tokens) as f64
                        * 100.0
                } else {
                    0.0
                }
            ),
            sample_tps: format!("{:.1}", grammar.sampled_tps()),
            sample_tokens: grammar.sampled_tokens,
            total_tps: format!("{:.1}", grammar.overall_tps()),
            total_time: format!(
                "{:.1}ms",
                grammar.injected_elapsed_ms + grammar.sampled_elapsed_ms
            ),
            forward_passes: grammar.forward_passes,
        },
        ResultRow {
            mode: "free-gen".to_string(),
            prompt_tps: format!("{:.1}", free.prompt_tps()),
            prompt_tokens: free.prompt_tokens,
            inject_tps: "-".to_string(),
            inject_tokens: 0,
            inject_pct: "-".to_string(),
            sample_tps: format!("{:.1}", free.sampled_tps()),
            sample_tokens: free.sampled_tokens,
            total_tps: format!("{:.1}", free.overall_tps()),
            total_time: format!("{:.1}ms", free.sampled_elapsed_ms),
            forward_passes: free.forward_passes,
        },
    ];

    println!();
    println!(
        "{}",
        tabled::Table::new(&rows).with(tabled::settings::Style::rounded())
    );

    // Speedup comparison
    if free.sampled_elapsed_ms > 0.0 && grammar.sampled_elapsed_ms > 0.0 {
        let grammar_sample_tps = grammar.sampled_tps();
        let free_sample_tps = free.sampled_tps();
        if free_sample_tps > 0.0 {
            let speedup = grammar_sample_tps / free_sample_tps;
            println!();
            println!(
                "Grammar sampling overhead: {:.2}x slower than free-gen ({:.1} t/s vs {:.1} t/s)",
                1.0 / speedup,
                free_sample_tps,
                grammar_sample_tps,
            );
        }
    }
}

fn main() {
    let args = Args::parse();

    if args.model.is_none() && args.model_dir.is_none() {
        eprintln!("Error: specify --model or --model-dir");
        std::process::exit(1);
    }

    let models = if let Some(model) = &args.model {
        vec![model.clone()]
    } else if let Some(dir) = &args.model_dir {
        find_gguf_models(dir)
    } else {
        unreachable!()
    };

    if models.is_empty() {
        eprintln!("No .gguf model files found!");
        std::process::exit(1);
    }

    eprintln!("Found {} model(s) to benchmark\n", models.len());

    for model_path in &models {
        run_model_benchmark(
            model_path,
            args.num_gpu_layers,
            args.max_ctx,
            args.max_tokens,
            args.runs,
        );
    }
}
