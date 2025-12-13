//! Module implementing some benchmarks, to evaluate how good of a job a certain model
//! will do based on already verified documents in paperless

use std::path::Path;

use paperless_api_client::{
    Client, custom_fields,
    types::{CustomField, Document},
};
use rand::{rng, seq::IteratorRandom};
use serde_json::Value;

use crate::{config::Config, extract::LLModelExtractor, requests};

#[derive(Debug, clap::Args)]
pub(crate) struct BenchmarkParameters {
    #[clap(long)]
    /// if set only document with this tag will be considered for benchmarking
    verified_docs_tag: Option<String>,

    #[clap(long)]
    /// amount of documents to select from corpus, if unspecified all docs will be used!
    sample_doc_size: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct SingleResult {
    doc_id: u64,
    expected_result: Value,
    benchmark_result: Value,
    success: bool,
}

#[derive(Debug)]
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
    client: &mut Client,
    results: &mut BenchmarkResults,
) {
    let valid_doc_state = doc.clone();

    let mut test_doc_state = doc.clone();
    test_doc_state.custom_fields = None;
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
            run_custom_field_benchmark(
                &mut model,
                doc,
                &custom_fields,
                &mut api_client,
                &mut benchmark_results,
            );
        }
    }
}
