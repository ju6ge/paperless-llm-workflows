//! Module implementing some benchmarks, to evaluate how good of a job a certain model
//! will do based on already verified documents in paperless

use std::{fs::File, io::Write, path::Path};

use itertools::Itertools;
use paperless_api_client::{
    Client, custom_fields,
    types::{CustomField, Document},
};
use rand::{rng, seq::IteratorRandom};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    config::Config,
    extract::{LLModelExtractor, ModelError},
    requests,
    types::{FieldError, FieldExtract, custom_field_learning_supported, schema_from_custom_field},
};

#[derive(Debug, clap::Args)]
pub(crate) struct BenchmarkParameters {
    #[clap(long)]
    /// if set only document with this tag will be considered for benchmarking
    verified_docs_tag: Option<String>,

    #[clap(long)]
    /// amount of documents to select from corpus, if unspecified all docs will be used!
    sample_doc_size: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SingleResult {
    doc_id: i64,
    expected_result: Value,
    benchmark_result: Value,
    success: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BenchmarkResults {
    custom_field_predictions: Vec<SingleResult>,
}

impl BenchmarkResults {
    pub fn init_empty() -> Self {
        Self {
            custom_field_predictions: Vec::new(),
        }
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
                                    results.custom_field_predictions.push(SingleResult {
                                        doc_id: valid_doc_state.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: true,
                                        error: None
                                    });
                                } else {
                                    results.custom_field_predictions.push(SingleResult {
                                        doc_id: valid_doc_state.id,
                                        expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                        benchmark_result: serde_json::to_value(extracted_cfi)
                                            .unwrap(),
                                        success: false,
                                        error: None
                                    });
                                }
                            }
                            Err(err) => {
                                results.custom_field_predictions.push(SingleResult {
                                    doc_id: valid_doc_state.id,
                                    expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                                    benchmark_result: serde_json::to_value(&field_extract).unwrap(),
                                    success: false,
                                    error: Some(err.to_string())
                                });
                            }
                        }
                    }
                    Err(model_err) => {
                        results.custom_field_predictions.push(SingleResult {
                            doc_id: valid_doc_state.id,
                            expected_result: serde_json::to_value(doc_cf.1).unwrap(),
                            benchmark_result: Value::Null,
                            success: false,
                            error: Some(model_err.to_string())
                        });
                    }
                }
            }
        }
    }
}

impl BenchmarkParameters {
    pub async fn run(&self, config: Config) {
        let mut api_client = Client::new_from_env();
        api_client.set_base_url(&config.paperless_server);

        let tags = requests::get_all_tags(&mut api_client).await;
        let custom_fields = requests::get_all_custom_fields(&mut api_client).await;
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

        let mut benchmark_results = BenchmarkResults::init_empty();
        let mut model =
            LLModelExtractor::new(Path::new(&config.model), config.num_gpu_layers, max_ctx)
                .expect("Language model is required to load for benchmarking its performance");

        for doc in &doc_to_process {
            // this function is the only task running, so we do not care that the benchmark functions may block for a long time
            run_custom_field_benchmark(&mut model, doc, &custom_fields, &mut benchmark_results);
        }

        //write results to disc
        let mut result_file = File::create("/tmp/paperless-llm-workflow-model-benchmark.json").unwrap();
        let _ = write!(&mut result_file, "{}", serde_json::to_string(&benchmark_results).unwrap());
    }
}
