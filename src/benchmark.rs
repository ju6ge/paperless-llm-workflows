use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};

use crossbeam_channel::Sender;
use paperless_api_client::{
    Client,
    types::{Correspondent, CustomField, Document},
};
use rand::{rng, seq::IteratorRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::VariantArray;
use tabled::{Table, Tabled, settings::Style};

use crate::{
    config::Config,
    extract::LLModelExtractor,
    requests,
    types::{
        Decision, FieldExtract, custom_field_learning_supported,
        schema_from_custom_field, schema_from_decision_question,
    },
};

#[derive(Debug, clap::Args)]
pub(crate) struct BenchmarkParameters {
    #[clap(long)]
    /// if set only document with this tag will be considered for benchmarking
    verified_docs_tag: Option<String>,

    #[clap(long)]
    /// amount of documents to select from corpus, if unspecified all docs will be used!
    sample_doc_size: Option<usize>,

    #[clap(long)]
    result_file: Option<String>,

    #[clap(long, default_value = "false", action)]
    view: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct MultiBenchmarkParameters {
    #[clap(long)]
    /// directory containing model files to benchmark
    model_directory: String,

    #[clap(long)]
    /// output directory for results
    output_directory: String,

    #[clap(long)]
    /// if set only document with this tag will be considered for benchmarking
    verified_docs_tag: Option<String>,

    #[clap(long)]
    /// amount of documents to select from corpus, if unspecified all docs will be used!
    sample_doc_size: Option<usize>,

    #[clap(long, default_value = "4")]
    /// number of parallel benchmark jobs
    jobs: usize,
}

#[derive(
    Debug, Serialize, Deserialize, strum::Display, strum::VariantArray, PartialEq, Eq, Clone,
)]
pub(crate) enum BenchmarkResultType {
    CustomFieldExtraction,
    CorrespondentSuggest,
    DecideValidCorrespondent,
    DecideInvalidCorrespondent,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct SingleResult {
    benchmark_type: BenchmarkResultType,
    doc_id: i64,
    expected_result: Value,
    benchmark_result: Value,
    success: bool,
    error: Option<String>,
}

#[derive(Tabled)]
pub(crate) struct BenchmarkKindSummary {
    benchmak_type: BenchmarkResultType,
    success: usize,
    failed: usize,
    errored: usize,
    success_rate: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct BenchmarkResults {
    model: String,
    results: Vec<SingleResult>,
}

impl BenchmarkResults {
    pub fn init_empty<S: ToString>(model_name: S) -> Self {
        Self {
            model: model_name.to_string(),
            results: Vec::new(),
        }
    }

    pub fn display_results(&self) {
        let mut table_rows = Vec::new();
        for benchmark_kind in BenchmarkResultType::VARIANTS {
            let succeded = self
                .results
                .iter()
                .filter(|r| r.benchmark_type == *benchmark_kind)
                .filter(|r| r.error.is_none())
                .filter(|r| r.success)
                .count();
            let failed = self
                .results
                .iter()
                .filter(|r| r.benchmark_type == *benchmark_kind)
                .filter(|r| r.error.is_none())
                .filter(|r| !r.success)
                .count();
            let errored = self
                .results
                .iter()
                .filter(|r| r.benchmark_type == *benchmark_kind)
                .filter(|r| r.error.is_some())
                .count();
            table_rows.push(BenchmarkKindSummary {
                benchmak_type: benchmark_kind.clone(),
                success: succeded,
                failed,
                errored,
                success_rate: format!(
                    "{:.2} %",
                    (succeded as f64) * 100.
                        / (self
                            .results
                            .iter()
                            .filter(|r| r.benchmark_type == *benchmark_kind)
                            .count() as f64)
                ),
            });
        }
        println!("{}", Table::new(table_rows).with(Style::ascii()));
    }
    pub fn current_stats(&self) {
        let succeded = self
            .results
            .iter()
            .filter(|r| r.error.is_none())
            .filter(|r| r.success)
            .count();
        let failed = self
            .results
            .iter()
            .filter(|r| r.error.is_none())
            .filter(|r| !r.success)
            .count();
        let errored = self
            .results
            .iter()
            .filter(|r| r.error.is_some())
            .count();
        let success_rate = (succeded as f64) / ( succeded + failed + errored ) as f64;
        println!("s:{succeded}|f:{failed}|e:{errored}|r:{success_rate}");
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ProgressUpdate {
    Started { model_name: String, total_docs: usize },
    DocumentProgress { doc_id: i64, progress: usize, total: usize },
    Completed { model_name: String, results: BenchmarkResults },
    Error { model_name: String, error: String },
}

struct BenchmarkContext<'a> {
    model: &'a mut LLModelExtractor,
    doc: &'a Document,
    custom_fields: &'a Vec<CustomField>,
    crrspndents: &'a Vec<Correspondent>,
    results: &'a mut BenchmarkResults,
    progress_sender: Option<Sender<ProgressUpdate>>,
}

fn run_custom_field_benchmark(
    ctx: &mut BenchmarkContext,
) {
    // unused variables - these are parameters that are not used in the function
    let valid_doc_state = ctx.doc.clone();
    let mut test_doc_state = ctx.doc.clone();
    // make sure custom fields are unpopulated for the document data used
    // for testing the extraction of custom fields
    test_doc_state.custom_fields = None;

    if let Some(valid_doc_cfs) = &valid_doc_state.custom_fields {
        for doc_cf in ctx.custom_fields
            .iter()
            .filter(|cf| custom_field_learning_supported(cf))
            .filter_map(|cf| {
                if let Some(d_cfi) = valid_doc_cfs.iter().find(|dcf| dcf.field == cf.id) {
                    Some((cf, d_cfi))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
        {
            if let Some(cf_grammar) = schema_from_custom_field(&doc_cf.0) {
                let doc_data = serde_json::to_value(&test_doc_state).unwrap();

                match ctx.model.extract(&doc_data, &cf_grammar, false) {
                    Ok(extracted_value) => {
                        let field_extract: FieldExtract = serde_json::from_value(extracted_value)
                            .expect("grammar forced output to match type");
                        match field_extract.to_custom_field_instance(&doc_cf.0) {
                            Ok(extracted_cfi) => {
                                if extracted_cfi == *doc_cf.1 {
                                    // the extracted value corresponds exactly to the value of the validated documen
                                    // so this is the only case that is considered a success
                                    ctx.results.results.push(SingleResult {
                                        benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                        doc_id: ctx.doc.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: true,
                                        error: None,
                                    });
                                } else {
                                    ctx.results.results.push(SingleResult {
                                        benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                        doc_id: ctx.doc.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: false,
                                        error: None,
                                    });
                                }
                            }
                            Err(err) => {
                                ctx.results.results.push(SingleResult {
                                    benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                    doc_id: ctx.doc.id,
                                    expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                    benchmark_result: serde_json::to_value(&field_extract).unwrap(),
                                    success: false,
                                    error: Some(err.to_string()),
                                });
                            }
                        }
                    }
                    Err(model_err) => {
                        ctx.results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                            doc_id: ctx.doc.id,
                            expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                            benchmark_result: Value::Null,
                            success: false,
                            error: Some(model_err.to_string()),
                        });
                    }
                }
            }
        }
    }
}

fn run_correspondent_suggest_benchmark(
    ctx: &mut BenchmarkContext,
) {
    let crrspndts_suggest_schema = crate::types::schema_from_correspondents(&ctx.crrspndents.as_slice());
    // unused variables - these are parameters that are not used in the function
    let doc_data = serde_json::to_value(&ctx.doc.content).unwrap();

    if let Some(expected_correspondent) = ctx.doc
        .correspondent
        .map(|dcr| ctx.crrspndents.iter().find(|c| c.id == dcr))
        .flatten()
    {
        match ctx.model.extract(&doc_data, &crrspndts_suggest_schema, false) {
            Ok(model_result_value) => {
                let field_extract: FieldExtract = serde_json::from_value(model_result_value)
                    .expect("grammar enforces output matches type");
                match field_extract.to_correspondent(&ctx.crrspndents.as_slice()) {
                    Ok(suggested_crrspndnt) => {
                        if suggested_crrspndnt.id == expected_correspondent.id {
                            ctx.results.results.push(SingleResult {
                                benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                                doc_id: ctx.doc.id,
                                expected_result: serde_json::to_value(
                                    expected_correspondent.name.clone(),
                                )
                                .unwrap(),
                                benchmark_result: serde_json::to_value(&suggested_crrspndnt.name)
                                    .unwrap(),
                                success: true,
                                error: None,
                            });
                        } else {
                            ctx.results.results.push(SingleResult {
                                benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                                doc_id: ctx.doc.id,
                                expected_result: serde_json::to_value(
                                    expected_correspondent.name.clone(),
                                )
                                .unwrap(),
                                benchmark_result: serde_json::to_value(&suggested_crrspndnt.name)
                                    .unwrap(),
                                success: false,
                                error: None,
                            });
                        }
                    }
                    Err(err) => {
                        ctx.results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                            doc_id: ctx.doc.id,
                            expected_result: serde_json::to_value(
                                expected_correspondent.name.clone(),
                            )
                            .unwrap(),
                            benchmark_result: serde_json::to_value(&field_extract).unwrap(),
                            success: false,
                            error: Some(err.to_string()),
                        });
                    }
                }
            }
            Err(model_error) => {
                ctx.results.results.push(SingleResult {
                    benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                    doc_id: ctx.doc.id,
                    expected_result: serde_json::to_value(expected_correspondent.name.clone())
                        .unwrap(),
                    benchmark_result: Value::Null,
                    success: false,
                    error: Some(model_error.to_string()),
                });
            }
        }
    } else {
        // for now documents without a correspondent are simply ignored
    }
}

/// this benchmark is used to evaluate true false questions based on the document
/// when adding new questions tests should always add tests for the positive and
/// the negative answer. Only questions where the validity can be checked based
/// on the document metadata programmatically may be used. This means only data that
/// is availible for every document, because otherwise this benchmark might become
/// very depenendent on the paperless instances configuration
fn run_decision_benchmarks(
    ctx: &mut BenchmarkContext,
) {
    let doc_data = serde_json::to_value(&ctx.doc.content).unwrap();

    // simple question is the correspondent correct, only if doc has correspondent!
    if let Some(expected_correspondent) = ctx.doc
        .correspondent
        .map(|dcr| ctx.crrspndents.iter().find(|c| c.id == dcr))
        .flatten()
    {
        let expected_yes_question = format!(
            "Is '{}' the author/sender of this document?",
            expected_correspondent.name
        );
        let question_schema = schema_from_decision_question(&expected_yes_question);
        match ctx.model.extract(&doc_data, &question_schema, false) {
            Ok(model_answer_value) => {
                let model_decision: Decision = serde_json::from_value(model_answer_value.clone())
                    .expect("grammar constrains output to match type");
                if model_decision.answer_bool {
                    ctx.results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                        doc_id: ctx.doc.id,
                        expected_result: Value::Bool(true),
                        benchmark_result: model_answer_value,
                        success: true,
                        error: None,
                    });
                } else {
                    ctx.results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                        doc_id: ctx.doc.id,
                        expected_result: Value::Bool(true),
                        benchmark_result: model_answer_value,
                        success: false,
                        error: None,
                    });
                }
            }
            Err(model_err) => {
                ctx.results.results.push(SingleResult {
                    benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                    doc_id: ctx.doc.id,
                    expected_result: Value::Bool(true),
                    benchmark_result: Value::Null,
                    success: false,
                    error: Some(model_err.to_string()),
                });
            }
        }

        // only if there is only one possible correspondent, then this case will not run, because there is no false correspondent to select from …
        if let Some(random_incorrect_correspondent) = ctx.crrspndents
            .iter()
            .filter(|c| c.id != expected_correspondent.id)
            .choose(&mut rng())
        {
            let expected_no_question = format!(
                "Is '{}' the author/sender of this document?",
                random_incorrect_correspondent.name
            );
            let question_schema = schema_from_decision_question(&expected_no_question);
            match ctx.model.extract(&doc_data, &question_schema, false) {
                Ok(model_answer_value) => {
                    let model_decision: Decision =
                        serde_json::from_value(model_answer_value.clone())
                            .expect("grammar constrains output to match type");
                    if !model_decision.answer_bool {
                        ctx.results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                            doc_id: ctx.doc.id,
                            expected_result: Value::Bool(false),
                            benchmark_result: model_answer_value,
                            success: true,
                            error: None,
                        });
                    } else {
                        ctx.results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                            doc_id: ctx.doc.id,
                            expected_result: Value::Bool(false),
                            benchmark_result: model_answer_value,
                            success: false,
                            error: None,
                        });
                    }
                }
                Err(model_err) => {
                    ctx.results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                        doc_id: ctx.doc.id,
                        expected_result: Value::Bool(false),
                        benchmark_result: Value::Null,
                        success: false,
                        error: Some(model_err.to_string()),
                    });
                }
            }
        }
    }
}

fn run_benchmark_for_document(
    model: &mut LLModelExtractor,
    doc: &Document,
    custom_fields: &Vec<CustomField>,
    crrspndents: &Vec<Correspondent>,
    results: &mut BenchmarkResults,
    progress_sender: Option<Sender<ProgressUpdate>>,
    doc_index: usize,
    total_docs: usize,
) {
    let mut ctx = BenchmarkContext {
        model,
        doc,
        custom_fields,
        crrspndents,
        results,
        progress_sender: progress_sender.clone(),
    };

    // Send progress update
    if let Some(sender) = &progress_sender {
        let _ = sender.send(ProgressUpdate::DocumentProgress {
            doc_id: doc.id,
            progress: doc_index,
            total: total_docs,
        });
    }

    // Run all benchmark types
    run_custom_field_benchmark(&mut ctx);
    run_correspondent_suggest_benchmark(&mut ctx);
    run_decision_benchmarks(&mut ctx);

    // Send stats update
    if let Some(sender) = &progress_sender {
        let _ = sender.send(ProgressUpdate::DocumentProgress {
            doc_id: doc.id,
            progress: doc_index + 1,
            total: total_docs,
        });
    }
}

impl MultiBenchmarkParameters {
    pub async fn run(&self, config: Config) {
        use std::fs;
        use walkdir::WalkDir;

        // Create output directory if it doesn't exist
        fs::create_dir_all(&self.output_directory)
            .expect("Failed to create output directory");

        // Find all .gguf files in the model directory
        let model_files = WalkDir::new(&self.model_directory)
            .max_depth(1)
            .into_iter()
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    if e.file_type().is_file() {
                        let path = e.path();
                        if let Some(ext) = path.extension() {
                            if ext.eq_ignore_ascii_case("gguf") {
                                return Some(path.to_path_buf());
                            }
                        }
                    }
                    None
                })
            })
            .collect::<Vec<_>>();

        if model_files.is_empty() {
            eprintln!("No .gguf model files found in {}", self.model_directory);
            return;
        }

        println!(
            "Found {} model(s) to benchmark",
            model_files.len()
        );

        // Prepare benchmark configuration
        let benchmark_config = BenchmarkConfig {
            verified_docs_tag: self.verified_docs_tag.clone(),
            sample_doc_size: self.sample_doc_size,
        };

        // Run benchmarks in parallel
        let mut handles = Vec::new();
        let model_files_arc = Arc::new(model_files);
        let output_dir_arc = Arc::new(self.output_directory.clone());
        let config_arc = Arc::new(config);
        let benchmark_config_arc = Arc::new(benchmark_config);

        // Pre-compute result names for all models
        let result_names: Vec<String> = model_files_arc.iter()
            .map(|model_file| self.filename_to_result_name(model_file))
            .collect();

        for (i, model_file) in model_files_arc.iter().enumerate() {
            let model_file = model_file.clone();
            let output_dir = output_dir_arc.clone();
            let config = config_arc.clone();
            let benchmark_config = benchmark_config_arc.clone();
            let result_name = result_names[i].clone();

            // Spawn benchmark task
            let handle = tokio::spawn(async move {
                let result_file = format!("{}/{}_results.json", output_dir, result_name);

                println!(
                    "Starting benchmark {}: {} -> {}",
                    i + 1,
                    model_file.display(),
                    result_file
                );

                let mut config_copy = (*config).clone();
                config_copy.model = model_file.to_string_lossy().into_owned();

                let params = BenchmarkParameters {
                    verified_docs_tag: benchmark_config.verified_docs_tag.clone(),
                    sample_doc_size: benchmark_config.sample_doc_size,
                    result_file: Some(result_file),
                    view: false,
                };

                params.run_with_progress(config_copy, None).await;
            });

            handles.push(handle);

            // Limit concurrent jobs
            if handles.len() >= self.jobs {
                // Wait for one job to complete
                let handle = handles.pop().unwrap();
                let result = handle.await;
                if let Err(e) = result {
                    eprintln!("Benchmark failed: {}", e);
                }
            }
        }

        // Wait for remaining jobs
        for handle in handles {
            let result = handle.await;
            if let Err(e) = result {
                eprintln!("Benchmark failed: {}", e);
            }
        }

        println!("All benchmarks completed!");
    }

    fn filename_to_result_name(&self, path: &Path) -> String {
        let filename = path.file_name().unwrap().to_string_lossy();
        let filename = filename.trim_end_matches(".gguf").trim_end_matches(".GGUF");
        
        // Convert to lowercase and replace common separators with hyphens
        let mut result_name = filename.to_lowercase();
        result_name = result_name.replace(|c: char| c == ' ' || c == '_' || c == '.' || c == '/', "-");
        result_name = result_name.replace("-", "-");
        
        // Remove multiple consecutive hyphens
        result_name = result_name.replace("---", "-");
        while result_name.contains("--") {
            result_name = result_name.replace("--", "-");
        }
        
        // Remove leading/trailing hyphens
        result_name.trim_matches('-').to_string()
    }
}

#[derive(Clone)]
struct BenchmarkConfig {
    verified_docs_tag: Option<String>,
    sample_doc_size: Option<usize>,
}

impl BenchmarkParameters {
    pub async fn run(&self, config: Config) {
        self.run_with_progress(config, None).await;
    }

    pub async fn run_with_progress(&self, config: Config, progress_sender: Option<Sender<ProgressUpdate>>) {
        if self.view {
            if let Some(result_file_path) = &self.result_file {
                let rfile = OpenOptions::new()
                    .read(true)
                    .open(result_file_path)
                    .unwrap();
                let benchmark_results: BenchmarkResults =
                    serde_json::from_reader(rfile).expect("Invalid benchmark result file!");
                benchmark_results.display_results();
            } else {
                println!("No result file path set no result to view! ... Exiting");
            }
            return ();
        }

        let mut api_client = Client::new_from_env();
        api_client.set_base_url(&config.paperless_server);

        let tags = requests::get_all_tags(&mut api_client).await;
        let custom_fields = requests::get_all_custom_fields(&mut api_client).await;
        let crrspndents = requests::fetch_all_correspondents(&mut api_client).await;
        let mut doc_to_process = requests::get_all_docs(&mut api_client)
            .await
            .into_iter()
            .filter(|doc| {
                if let Some(verified_tag_name) = &self.verified_docs_tag
                    && let Some(verified_tag) =
                        tags.iter().find(|tag| tag.name == *verified_tag_name)
                {
                    doc.tags.contains(&verified_tag.id)
                } else {
                    // verified tag unspecified or does not exist falling back to use all docs without inbox tags
                    tags.iter()
                        // filter tags to only the ones of the document
                        .filter(|tag| doc.tags.contains(&tag.id))
                        // check to find if any of the tags is an inbox tag
                        .find(|tag| tag.is_inbox_tag.is_some_and(|inbox| inbox))
                        // if no tag is found, the document can be used for benchmarking
                        .is_none()
                }
            })
            .collect::<Vec<_>>();
        if let Some(sample_size) = self.sample_doc_size {
            doc_to_process = doc_to_process
                .into_iter()
                .choose_multiple(&mut rng(), sample_size);
        }

        let max_ctx = if config.max_ctx == 0 {
            None
        } else {
            Some(config.max_ctx as u32)
        };

        let mut benchmark_results = BenchmarkResults::init_empty(&config.model);
        let mut model =
            LLModelExtractor::new(Path::new(&config.model), config.num_gpu_layers, max_ctx)
                .expect("Language model is required to load for benchmarking its performance");

        // Send start notification
        if let Some(sender) = &progress_sender {
            let _ = sender.send(ProgressUpdate::Started {
                model_name: config.model.clone(),
                total_docs: doc_to_process.len(),
            });
        }

        for (i, doc) in doc_to_process.iter().enumerate() {
            run_benchmark_for_document(
                &mut model,
                doc,
                &custom_fields,
                &crrspndents,
                &mut benchmark_results,
                progress_sender.clone(),
                i,
                doc_to_process.len(),
            );
        }

        // Send completion notification
        if let Some(sender) = &progress_sender {
            let _ = sender.send(ProgressUpdate::Completed {
                model_name: config.model.clone(),
                results: benchmark_results.clone(),
            });
        }

        //write results to disc
        if let Some(result_file_path) = &self.result_file {
            let mut result_file = File::create(result_file_path).unwrap();
            let _ = write!(
                &mut result_file,
                "{}",
                serde_json::to_string(&benchmark_results).unwrap()
            );
        }

        benchmark_results.display_results();
    }
}
