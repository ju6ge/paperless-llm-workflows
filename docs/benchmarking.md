# Benchmarking

The benchmarking system lets you evaluate and compare language models against your actual paperless-ngx documents. It measures how accurately models extract custom fields, suggest correspondents, and make decisions based on document content.

> **Note**: Benchmarking is a build-time feature. The prebuilt containers do not include benchmarking support. Build from source with `--features benchmark` to enable it.

## Building with Benchmarking

```bash
cargo build --release -F vulkan -F benchmark
```

## Running Benchmarks

### Prerequisites

- A working paperless-ngx instance with documents
- Documents with verified metadata (custom fields, correspondents, tags)
- Build the binary with the `benchmark` feature enabled

### Single Model Benchmarking

Evaluate one model against your documents:

```bash
./target/release/paperless-llm-workflows benchmark \
    --paperless-server https://your-paperless.instance \
    --model /path/to/your/model.gguf \
    --verified-docs-tag "verified" \
    --result-file results.json
```

**Options**:
| Flag | Description |
|---|---|
| `--paperless-server` | URL of your paperless-ngx instance |
| `--model` | Path to GGUF model file |
| `--verified-docs-tag` | Only benchmark documents with this tag (recommended) |
| `--sample-doc-size` | Limit to N documents (default: all verified) |
| `--result-file` | Save results to JSON file |
| `--view` | Display previously saved results from a file |

### Multi-Model Benchmarking

Compare multiple models in parallel:

```bash
./target/release/paperless-llm-workflows multi-benchmark \
    --paperless-server https://your-paperless.instance \
    --model-directory /path/to/models/ \
    --output-directory /path/to/results/ \
    --verified-docs-tag "verified" \
    --jobs 4
```

This will:
1. Find all `.gguf` files in the model directory
2. Run benchmarks on each model in parallel (up to `--jobs` concurrent)
3. Save individual results for each model
4. Display a summary comparison

### Viewing Results

Results are displayed in a real-time TUI interface showing:
- Progress bars for each model
- Overall progress across all models
- Success/failure/error counts
- Success rates by benchmark type

```bash
./target/release/paperless-llm-workflows benchmark --view --result-file results.json
```

## Understanding Results

The benchmark tracks three metrics per test:

| Metric | Meaning |
|---|---|
| **Success** | Model produced the correct answer |
| **Failed** | Model produced an incorrect answer |
| **Errored** | Model encountered an error during processing |

### Benchmark Test Types

1. **Custom Field Extraction** — Tests the model's ability to extract and fill custom fields from document content. The model must match the exact value that was previously verified for the document.

2. **Correspondent Suggestion** — Tests the model's ability to suggest the correct correspondent (author/sender) based on document content. The suggestion is compared against the verified correspondent.

3. **Decision Making** — Tests the model's reasoning with true/false questions:
   - "Is [verified correspondent] the author/sender of this document?" (expected: true)
   - "Is [random correspondent] the author/sender of this document?" (expected: false)

Success rates are calculated per benchmark type and overall.

## Result Format

Results are saved as JSON files containing:
- Model name
- Document ID
- Expected result
- Actual benchmark result
- Success/failure status
- Error messages (if any)

## Analysis Scripts

The `scripts/` directory contains Python tools for analyzing benchmark results:

### Histogram Comparison

```bash
python scripts/eval-bench-results.py ../benchmark_results --output_plot "out.png"
```

Produces `out.png` with a histogram of success, failure, and error counts per benchmark type.

### Comparison Table

```bash
python scripts/eval-bench-results.py ../benchmark_results --output_json "eval.json"
typst compile scripts/model-comparision-table.typ --input benchmark_stats=eval.json table.svg
```

Produces a table of model success rates ordered by overall metric.
