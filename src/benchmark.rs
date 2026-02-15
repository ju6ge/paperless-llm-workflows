use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use futures::{FutureExt, select};
use futures_timer::Delay;
use paperless_api_client::{
    Client,
    types::{Correspondent, CustomField, Document},
};
use rand::{rng, seq::IteratorRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::VariantArray;
use tabled::{Table, Tabled, settings::Style};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::RwLock,
    time::Duration,
};

use crate::{
    config::Config,
    extract::LLModelExtractor,
    requests,
    tui::BenchmarkApp,
    types::{
        Decision, FieldExtract, custom_field_learning_supported, schema_from_custom_field,
        schema_from_decision_question,
    },
};

#[derive(Debug, clap::Args, Serialize, Deserialize)]
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

#[derive(Debug, clap::Args, Serialize, Deserialize)]
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
        let errored = self.results.iter().filter(|r| r.error.is_some()).count();
        let success_rate = (succeded as f64) / (succeded + failed + errored) as f64;
        println!("s:{succeded}|f:{failed}|e:{errored}|r:{success_rate}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) enum ProgressUpdate {
    /// register benchmark run with model
    ///
    /// depending on the amount of parallel benchmark runs not all benchmarks will be running at the same time
    /// so in order for the benchmark ui to know how many results to expect each future run is registered at the beginning
    Register {
        model_name: String,
        total_docs: usize,
    },
    /// benchmark subprocess for model has started
    Started {
        model_name: String,
        total_docs: usize,
    },
    /// current benchmark progress update
    DocumentProgress {
        model_name: String,
        doc_id: i64,
        progress: usize,
        total: usize,
    },
    /// intermediary or final benchmark result state
    BenchmarkResults {
        model_name: String,
        results: BenchmarkResults,
    },
    /// error report
    Error { model_name: String, error: String },
}

struct BenchmarkContext<'a> {
    model: &'a mut LLModelExtractor,
    doc: &'a Document,
    custom_fields: &'a Vec<CustomField>,
    crrspndents: &'a Vec<Correspondent>,
    results: &'a mut BenchmarkResults,
}

fn run_custom_field_benchmark(ctx: &mut BenchmarkContext) {
    // unused variables - these are parameters that are not used in the function
    let valid_doc_state = ctx.doc.clone();
    let mut test_doc_state = ctx.doc.clone();
    // make sure custom fields are unpopulated for the document data used
    // for testing the extraction of custom fields
    test_doc_state.custom_fields = None;

    if let Some(valid_doc_cfs) = &valid_doc_state.custom_fields {
        for doc_cf in ctx
            .custom_fields
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

fn run_correspondent_suggest_benchmark(ctx: &mut BenchmarkContext) {
    let crrspndts_suggest_schema =
        crate::types::schema_from_correspondents(&ctx.crrspndents.as_slice());
    // unused variables - these are parameters that are not used in the function
    let doc_data = serde_json::to_value(&ctx.doc.content).unwrap();

    if let Some(expected_correspondent) = ctx
        .doc
        .correspondent
        .map(|dcr| ctx.crrspndents.iter().find(|c| c.id == dcr))
        .flatten()
    {
        match ctx
            .model
            .extract(&doc_data, &crrspndts_suggest_schema, false)
        {
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
fn run_decision_benchmarks(ctx: &mut BenchmarkContext) {
    let doc_data = serde_json::to_value(&ctx.doc.content).unwrap();

    // simple question is the correspondent correct, only if doc has correspondent!
    if let Some(expected_correspondent) = ctx
        .doc
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
        if let Some(random_incorrect_correspondent) = ctx
            .crrspndents
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
    model_name: &String,
    model: &mut LLModelExtractor,
    doc: &Document,
    custom_fields: &Vec<CustomField>,
    crrspndents: &Vec<Correspondent>,
    results: &mut BenchmarkResults,
    log_to_stdio: bool,
    doc_index: usize,
    total_docs: usize,
) {
    let mut ctx = BenchmarkContext {
        model,
        doc,
        custom_fields,
        crrspndents,
        results,
    };

    // Run all benchmark types
    run_custom_field_benchmark(&mut ctx);
    run_correspondent_suggest_benchmark(&mut ctx);
    run_decision_benchmarks(&mut ctx);

    // Send stats update
    if log_to_stdio {
        let _ = writeln!(
            std::io::stdout(),
            "{}",
            serde_json::to_string(&ProgressUpdate::DocumentProgress {
                model_name: model_name.to_string(),
                doc_id: doc.id,
                progress: doc_index + 1,
                total: total_docs,
            })
            .unwrap()
        );
    }
}

impl MultiBenchmarkParameters {
    async fn start_benchmark_worker(
        config: Config,
        doc_to_process: Vec<Document>,
        custom_fields: Vec<CustomField>,
        crrspndents: Vec<Correspondent>,
        _model_name: String,
        result_path: PathBuf,
        progress_sender: tokio::sync::mpsc::UnboundedSender<ProgressUpdate>,
        shared_running_flag: Arc<tokio::sync::RwLock<bool>>,
    ) -> tokio::task::JoinHandle<()> {
        let ownbinary = env::current_exe()
            .expect("failed to get current executable path")
            .display()
            .to_string();
        let mut child = Command::new(ownbinary)
            .arg("benchmark-worker")
            .stdout(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to start benchmark process");

        let mut stdin = child.stdin.take().expect("failed to open stdin");
        let benchmark_run_data = serde_json::to_string(&BenchmarkWorkerData {
            config,
            doc_to_process,
            custom_fields,
            crrspndents,
        })
        .unwrap();
        let _ = stdin.write_all(benchmark_run_data.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        let _ = stdin.flush().await;

        let stdout = child.stdout.take().expect("failed to open stdout");
        let mut stdout_reader = BufReader::new(stdout).lines();

        let shared_suprocess_running_flag = Arc::new(RwLock::new(true));
        let shared_suprocess_running_flag_2 = shared_running_flag.clone();
        let shared_running_flag_2 = shared_running_flag.clone();
        let progress_sender_clone = progress_sender.clone();
        let result_path_clone = result_path.clone();

        let process_receiver = tokio::spawn(async move {
            let mut results_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&result_path_clone)
                .expect("Failed to create results file");

            while let Ok(maybe_line) = stdout_reader.next_line().await
                && *shared_running_flag.read().await
                && *shared_suprocess_running_flag_2.read().await
            {
                if let Some(line) = maybe_line {
                    if let Ok(update) = serde_json::from_str::<ProgressUpdate>(&line) {
                        let _ = progress_sender_clone.send(update.clone());

                        // Save results to file when we receive BenchmarkResults update
                        if let ProgressUpdate::BenchmarkResults { results, .. } = update {
                            let _ = serde_json::to_writer(&mut results_file, &results)
                                .expect("Failed to write results to file");
                            let _ = results_file
                                .sync_all()
                                .expect("Failed to sync results file");
                        }
                    }
                }
            }
        });

        let process_watchdog = tokio::spawn(async move {
            let mut kill_subprocess = false;
            while *shared_suprocess_running_flag.read().await {
                if kill_subprocess {
                    let _ = child.kill().await;
                    break;
                }
                let mut delay = Delay::new(Duration::from_millis(500)).fuse();
                let mut child_finished = child.wait().boxed().fuse();
                select! {
                    _ = delay => {
                        if *shared_running_flag_2.read().await == false {
                            kill_subprocess = true;
                        }
                    }
                    _subprocess_finished = child_finished => {
                        *shared_suprocess_running_flag.write().await = false;
                    }
                }
            }
        });

        tokio::spawn(async move {
            let _ = tokio::join!(process_receiver, process_watchdog);
        })
    }

    pub async fn run_tui(&self, config: Config) {
        use std::fs;
        use walkdir::WalkDir;

        // Create output directory if it doesn't exist
        fs::create_dir_all(&self.output_directory).expect("Failed to create output directory");

        // Find all .gguf files in the model directory
        let model_files = WalkDir::new(&self.model_directory)
            .max_depth(1)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    if e.file_type().is_file() {
                        let path = e.path();
                        if let Some(ext) = path.extension() {
                            if ext.eq_ignore_ascii_case("gguf") {
                                //eprintln!("{path:?}");
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

        let (custom_fields, crrspndents, doc_to_process) =
            get_benchmark_data_from_paperless_instance(
                &config,
                &self.verified_docs_tag,
                &self.sample_doc_size,
            )
            .await;

        // Initialize TUI for multi-benchmark
        let shared_running_flag = Arc::new(tokio::sync::RwLock::new(true));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let terminal = ratatui::init();
        let tui = BenchmarkApp::new(shared_running_flag.clone(), rx).run(terminal);

        // Start benchmark processes for each model
        // Note: jobs parameter limits parallel subprocesses, but each model processes ALL documents
        let mut handles = Vec::new();
        let mut all_results = Vec::new();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.jobs));

        for model_file in &model_files {
            let model_name = self.filename_to_result_name(model_file);
            let result_path =
                Path::new(&self.output_directory).join(format!("{}.json", model_name));

            // Create config for this model
            let mut model_config = config.clone();
            model_config.model = model_file.to_string_lossy().to_string();

            // Each model processes ALL documents
            let docs_for_model = doc_to_process.clone();

            // Send register update for TUI
            let tx_clone = tx.clone();
            let _ = tx_clone.send(ProgressUpdate::Register {
                model_name: model_name.clone(),
                total_docs: docs_for_model.len(),
            });

            // Start benchmark worker with semaphore
            let semaphore_clone = semaphore.clone();
            let custom_fields_clone = custom_fields.clone();
            let crrspndents_clone = crrspndents.clone();
            let shared_running_flag_clone = shared_running_flag.clone();
            let model_name_clone = model_name.clone();
            let result_path_clone = result_path.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore_clone.acquire().await;
                Self::start_benchmark_worker(
                    model_config,
                    docs_for_model,
                    custom_fields_clone,
                    crrspndents_clone,
                    model_name_clone,
                    result_path_clone,
                    tx_clone,
                    shared_running_flag_clone,
                )
                .await
            });

            handles.push(handle);
            all_results.push((model_name, result_path));
        }

        // Wait for all benchmarks to complete
        for handle in handles {
            let _ = handle.await;
        }

        // Wait for TUI to finish
        let _ = tui.await;
        ratatui::restore();

        // Display summary of all results
        for (model_name, result_path) in all_results {
            if let Ok(results_file) = std::fs::File::open(&result_path) {
                let benchmark_results: Result<BenchmarkResults, _> =
                    serde_json::from_reader(results_file);
                if let Ok(results) = benchmark_results {
                    println!("\n=== Results for {} ===", model_name);
                    results.display_results();
                }
            }
        }
    }

    fn filename_to_result_name(&self, path: &Path) -> String {
        let filename = path.file_name().unwrap().to_string_lossy();
        let filename = filename.trim_end_matches(".gguf").trim_end_matches(".GGUF");

        // Convert to lowercase and replace common separators with hyphens
        let mut result_name = filename.to_lowercase();
        result_name =
            result_name.replace(|c: char| c == ' ' || c == '_' || c == '.' || c == '/', "-");
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

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkWorkerData {
    config: Config,
    doc_to_process: Vec<Document>,
    custom_fields: Vec<CustomField>,
    crrspndents: Vec<Correspondent>,
}

/// this function implements a benchmark worker as it's own subprocess
///
/// benchmarking multiple models in parallell requires multiple instances of llama.cpp. This is only possible when running llama.cpp in multiple isolated processes
pub(crate) fn benchmark_worker() {
    let mut benchmark_job_str = String::new();
    let _ = std::io::stdin().read_line(&mut benchmark_job_str);
    let benchmark_job_data: BenchmarkWorkerData = serde_json::from_str(&benchmark_job_str).unwrap();

    let max_ctx = if benchmark_job_data.config.max_ctx == 0 {
        None
    } else {
        Some(benchmark_job_data.config.max_ctx as u32)
    };

    let mut benchmark_results = BenchmarkResults::init_empty(&benchmark_job_data.config.model);
    let mut model = LLModelExtractor::new(
        Path::new(&benchmark_job_data.config.model),
        benchmark_job_data.config.num_gpu_layers,
        max_ctx,
    )
    .expect("Language model is required to load for benchmarking its performance");

    // Send start notification
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        serde_json::to_string(&ProgressUpdate::Started {
            model_name: benchmark_job_data.config.model.clone(),
            total_docs: benchmark_job_data.doc_to_process.len(),
        })
        .unwrap()
    );

    for (i, doc) in benchmark_job_data.doc_to_process.iter().enumerate() {
        run_benchmark_for_document(
            &benchmark_job_data.config.model,
            &mut model,
            doc,
            &benchmark_job_data.custom_fields,
            &benchmark_job_data.crrspndents,
            &mut benchmark_results,
            true,
            i,
            benchmark_job_data.doc_to_process.len(),
        );
    }

    // Send completion notification
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        serde_json::to_string(&ProgressUpdate::BenchmarkResults {
            model_name: benchmark_job_data.config.model.clone(),
            results: benchmark_results.clone(),
        })
        .unwrap()
    );
}

async fn get_benchmark_data_from_paperless_instance(
    config: &Config,
    verified_docs_tag: &Option<String>,
    sample_size: &Option<usize>,
) -> (Vec<CustomField>, Vec<Correspondent>, Vec<Document>) {
    let mut api_client = Client::new_from_env();
    api_client.set_base_url(&config.paperless_server);

    let tags = requests::get_all_tags(&mut api_client).await;
    let custom_fields = requests::get_all_custom_fields(&mut api_client).await;
    let crrspndents = requests::fetch_all_correspondents(&mut api_client).await;
    let mut doc_to_process = requests::get_all_docs(&mut api_client)
        .await
        .into_iter()
        .filter(|doc| {
            if let Some(verified_tag_name) = verified_docs_tag
                && let Some(verified_tag) = tags.iter().find(|tag| tag.name == *verified_tag_name)
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
    if let Some(sample_size) = sample_size {
        doc_to_process = doc_to_process
            .into_iter()
            .choose_multiple(&mut rng(), *sample_size);
    }
    (custom_fields, crrspndents, doc_to_process)
}

impl BenchmarkParameters {
    pub async fn run_tui(&self, config: Config) {
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
        let shared_running_flag = Arc::new(tokio::sync::RwLock::new(true));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let terminal = ratatui::init();
        let tui = BenchmarkApp::new(shared_running_flag.clone(), rx).run(terminal);
        let benchmark = self.run_with_progress(config, Some(tx), shared_running_flag);
        let _ = tokio::join!(tui, benchmark);
        ratatui::restore();
    }

    pub async fn run_with_progress(
        &self,
        config: Config,
        mut progress_sender: Option<tokio::sync::mpsc::UnboundedSender<ProgressUpdate>>,
        shared_running_flag: Arc<tokio::sync::RwLock<bool>>,
    ) {
        let (custom_fields, crrspndents, doc_to_process) =
            get_benchmark_data_from_paperless_instance(
                &config,
                &self.verified_docs_tag,
                &self.sample_doc_size,
            )
            .await;
        if let Some(progress_channel) = progress_sender.as_mut() {
            let _ = progress_channel.send(ProgressUpdate::Register {
                model_name: config.model.clone(),
                total_docs: doc_to_process.len(),
            });
        }

        let ownbinary = env::current_exe()
            .expect("failed to get current executable path")
            .display()
            .to_string();
        let mut child = Command::new(ownbinary)
            .arg("benchmark-worker")
            .stdout(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to start benchmark process");

        let mut stdin = child.stdin.take().expect("failed to open stdin");
        let benchmark_run_data = serde_json::to_string(&BenchmarkWorkerData {
            config,
            doc_to_process,
            custom_fields,
            crrspndents,
        })
        .unwrap();
        let _ = stdin.write_all(benchmark_run_data.as_bytes()).await;
        let _ = stdin.write_all(b"\n").await;
        let _ = stdin.flush().await;

        let stdout = child.stdout.take().expect("failed to open stdout");
        let mut stdout_reader = BufReader::new(stdout).lines();

        let shared_suprocess_running_flag = Arc::new(RwLock::new(true));
        let shared_suprocess_running_flag_2 = shared_running_flag.clone();
        let shared_running_flag_2 = shared_running_flag.clone();

        let process_receiver = tokio::spawn(async move {
            while let Ok(maybe_line) = stdout_reader.next_line().await
                && *shared_running_flag.read().await
                && *shared_suprocess_running_flag_2.read().await
            {
                if let Some(progress_channel) = progress_sender.as_mut()
                    && let Some(line) = maybe_line
                {
                    if let Ok(update) = serde_json::from_str::<ProgressUpdate>(&line) {
                        let _ = progress_channel.send(update);
                    }
                }
            }
        });

        let process_watchdog = tokio::spawn(async move {
            let mut kill_subprocess = false;
            while *shared_suprocess_running_flag.read().await {
                if kill_subprocess {
                    let _ = child.kill().await;
                    break;
                }
                let mut delay = Delay::new(Duration::from_millis(500)).fuse();
                let mut child_finished = child.wait().boxed().fuse();
                select! {
                    _ = delay => {
                        if *shared_running_flag_2.read().await == false {
                            kill_subprocess = true;
                        }
                    }
                    _subprocess_finishd = child_finished => {
                        *shared_suprocess_running_flag.write().await = false;
                    }
                }
            }
        });

        let _ = tokio::join!(process_receiver, process_watchdog);
    }
}
