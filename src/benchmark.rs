//! Module implementing some benchmarks, to evaluate how good of a job a certain model
//! will do based on already verified documents in paperless

use std::{fs::{File, OpenOptions}, io::Write, path::Path};

use indicatif::ProgressBar;
use itertools::Itertools;
use paperless_api_client::{
    Client, custom_fields,
    types::{Correspondent, CustomField, Document},
};
use rand::{rng, seq::IteratorRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::VariantArray;
use tabled::{Table, Tabled, builder::Builder, settings::Style};

use crate::{
    config::Config,
    extract::{LLModelExtractor, ModelError},
    requests,
    types::{
        Decision, FieldError, FieldExtract, custom_field_learning_supported,
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
    view: bool
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

#[derive(Debug, Serialize, Deserialize)]
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
}

#[derive(Debug, Serialize, Deserialize)]
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
                .filter(|r| r.success)
                .count();
            let failed = self
                .results
                .iter()
                .filter(|r| r.benchmark_type == *benchmark_kind)
                .filter(|r| !r.success)
                .count();
            table_rows.push(BenchmarkKindSummary {
                benchmak_type: benchmark_kind.clone(),
                success: succeded,
                failed,
            });
        }
        println!("{}", Table::new(table_rows).with(Style::ascii()));
    }
}

fn run_custom_field_benchmark(
    model: &mut LLModelExtractor,
    doc: &Document,
    custom_fields: &Vec<CustomField>,
    results: &mut BenchmarkResults,
) {
    let valid_doc_state = doc.clone();

    let mut test_doc_state = doc.clone();
    // make sure custom fields are unpopulated for the document data used
    // for testing the extraction of custom fields
    test_doc_state.custom_fields = None;

    if let Some(valid_doc_cfs) = &valid_doc_state.custom_fields {
        for doc_cf in custom_fields
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

                match model.extract(&doc_data, &cf_grammar, false) {
                    Ok(extracted_value) => {
                        let field_extract: FieldExtract = serde_json::from_value(extracted_value)
                            .expect("grammar forced output to match type");
                        match field_extract.to_custom_field_instance(&doc_cf.0) {
                            Ok(extracted_cfi) => {
                                if extracted_cfi == *doc_cf.1 {
                                    // the extracted value corresponds exactly to the value of the validated documen
                                    // so this is the only case that is considered a success
                                    results.results.push(SingleResult {
                                        benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                        doc_id: valid_doc_state.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: true,
                                        error: None,
                                    });
                                } else {
                                    results.results.push(SingleResult {
                                        benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                        doc_id: valid_doc_state.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: false,
                                        error: None,
                                    });
                                }
                            }
                            Err(err) => {
                                results.results.push(SingleResult {
                                    benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                                    doc_id: valid_doc_state.id,
                                    expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                    benchmark_result: serde_json::to_value(&field_extract).unwrap(),
                                    success: false,
                                    error: Some(err.to_string()),
                                });
                            }
                        }
                    }
                    Err(model_err) => {
                        results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::CustomFieldExtraction,
                            doc_id: valid_doc_state.id,
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
    model: &mut LLModelExtractor,
    doc: &Document,
    crrspndnts: &Vec<Correspondent>,
    results: &mut BenchmarkResults,
) {
    let crrspndts_suggest_schema = crate::types::schema_from_correspondents(&crrspndnts.as_slice());
    let doc_data = serde_json::to_value(&doc.content).unwrap();

    if let Some(expected_correspondent) = doc
        .correspondent
        .map(|dcr| crrspndnts.iter().find(|c| c.id == dcr))
        .flatten()
    {
        match model.extract(&doc_data, &crrspndts_suggest_schema, false) {
            Ok(model_result_value) => {
                let field_extract: FieldExtract = serde_json::from_value(model_result_value)
                    .expect("grammar enforces output matches type");
                match field_extract.to_correspondent(&crrspndnts.as_slice()) {
                    Ok(suggested_crrspndnt) => {
                        if suggested_crrspndnt.id == expected_correspondent.id {
                            results.results.push(SingleResult {
                                benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                                doc_id: doc.id,
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
                            results.results.push(SingleResult {
                                benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                                doc_id: doc.id,
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
                        results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                            doc_id: doc.id,
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
                results.results.push(SingleResult {
                    benchmark_type: BenchmarkResultType::CorrespondentSuggest,
                    doc_id: doc.id,
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
    model: &mut LLModelExtractor,
    doc: &Document,
    crrspndnts: &Vec<Correspondent>,
    results: &mut BenchmarkResults,
) {
    let doc_data = serde_json::to_value(&doc.content).unwrap();

    // simple question is the correspondent correct, only if doc has correspondent!
    if let Some(expected_correspondent) = doc
        .correspondent
        .map(|dcr| crrspndnts.iter().find(|c| c.id == dcr))
        .flatten()
    {
        let expected_yes_question = format!(
            "Is '{}' the author/sender of this document?",
            expected_correspondent.name
        );
        let question_schema = schema_from_decision_question(&expected_yes_question);
        match model.extract(&doc_data, &question_schema, false) {
            Ok(model_answer_value) => {
                let model_decision: Decision = serde_json::from_value(model_answer_value.clone())
                    .expect("grammar constrains output to match type");
                if model_decision.answer_bool {
                    results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                        doc_id: doc.id,
                        expected_result: Value::Bool(true),
                        benchmark_result: model_answer_value,
                        success: true,
                        error: None,
                    });
                } else {
                    results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                        doc_id: doc.id,
                        expected_result: Value::Bool(true),
                        benchmark_result: model_answer_value,
                        success: false,
                        error: None,
                    });
                }
            }
            Err(model_err) => {
                results.results.push(SingleResult {
                    benchmark_type: BenchmarkResultType::DecideValidCorrespondent,
                    doc_id: doc.id,
                    expected_result: Value::Bool(true),
                    benchmark_result: Value::Null,
                    success: false,
                    error: Some(model_err.to_string()),
                });
            }
        }

        // only if there is only one possible correspondent, then this case will not run, because there is no false correspondent to select from …
        if let Some(random_incorrect_correspondent) = crrspndnts
            .iter()
            .filter(|c| c.id != expected_correspondent.id)
            .choose(&mut rng())
        {
            let expected_no_question = format!(
                "Is '{}' the author/sender of this document?",
                random_incorrect_correspondent.name
            );
            let question_schema = schema_from_decision_question(&expected_no_question);
            match model.extract(&doc_data, &question_schema, false) {
                Ok(model_answer_value) => {
                    let model_decision: Decision =
                        serde_json::from_value(model_answer_value.clone())
                            .expect("grammar constrains output to match type");
                    if !model_decision.answer_bool {
                        results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                            doc_id: doc.id,
                            expected_result: Value::Bool(false),
                            benchmark_result: model_answer_value,
                            success: true,
                            error: None,
                        });
                    } else {
                        results.results.push(SingleResult {
                            benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                            doc_id: doc.id,
                            expected_result: Value::Bool(false),
                            benchmark_result: model_answer_value,
                            success: false,
                            error: None,
                        });
                    }
                }
                Err(model_err) => {
                    results.results.push(SingleResult {
                        benchmark_type: BenchmarkResultType::DecideInvalidCorrespondent,
                        doc_id: doc.id,
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

impl BenchmarkParameters {
    pub async fn run(&self, config: Config) {
        if self.view {
            if let Some(result_file_path) = &self.result_file {
                let rfile = OpenOptions::new().read(true).open(result_file_path).unwrap();
                let benchmark_results: BenchmarkResults = serde_json::from_reader(rfile).expect("Invalid benchmark result file!");
                benchmark_results.display_results();
            } else {
                println!("No result file path set no result to view! … Exiting")
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

        let pb = ProgressBar::new(doc_to_process.len() as u64);
        for doc in &doc_to_process {
            pb.set_message(format!(
                "Performing Model benchmarks for document with id {}",
                doc.id
            ));
            // this function is the only task running, so we do not care that the benchmark functions may block for a long time
            run_custom_field_benchmark(&mut model, doc, &custom_fields, &mut benchmark_results);
            run_correspondent_suggest_benchmark(
                &mut model,
                doc,
                &crrspndents,
                &mut benchmark_results,
            );
            run_decision_benchmarks(&mut model, doc, &crrspndents, &mut benchmark_results);
            pb.inc(1);
        }
        pb.finish_with_message("All Documents processed, displaying model performance results!");

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
