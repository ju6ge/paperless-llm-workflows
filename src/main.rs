use std::{path::Path, process::exit};

use benchmark::{BenchmarkParameters, MultiBenchmarkParameters};
use clap::Parser;
use config::{Config, OverlayConfig};
use paperless_api_client::Client;
use server::run_server;
use utoipa::OpenApi;

mod benchmark;
mod config;
mod extract;
mod requests;
mod server;
mod types;

#[cfg(any(
    all(feature = "vulkan", feature = "openmp"),
    all(feature = "vulkan", feature = "cuda"),
    all(feature = "openmp", feature = "cuda"),
))]
compile_error!(
    "Only one compute backend can be used, choose feature `vulkan`, `openmp`, or `cuda`!"
);

#[cfg(not(any(feature = "vulkan", feature = "openmp", feature = "cuda")))]
compile_error!(
    "Choose feature `vulkan`, `openmp`, or `cuda` to select what compute backend should be used for inference!"
);

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Args {
    #[clap(subcommand)]
    action: Action,

    /// Listen address of service
    #[clap(long, global = true)]
    host: Option<String>,

    /// Listen port of service
    #[clap(long, global = true)]
    port: Option<u16>,

    /// URL of the paperless instance
    #[clap(long, global = true)]
    paperless_server: Option<String>,

    /// Path to the GGUF model file
    #[clap(long, global = true)]
    model: Option<String>,

    /// Number of GPU layers for inference (0 = unlimited)
    #[clap(long, global = true)]
    num_gpu_layers: Option<usize>,

    /// Maximum context size for inference (0 = use model default)
    #[clap(long, global = true)]
    max_ctx: Option<usize>,

    /// Display name of the processing tag
    #[clap(long, global = true)]
    processing_tag: Option<String>,

    /// Display color of the processing tag
    #[clap(long, global = true)]
    processing_color: Option<String>,

    /// Display name of the finished tag
    #[clap(long, global = true)]
    finished_tag: Option<String>,

    /// Display color of the finished tag
    #[clap(long, global = true)]
    finished_color: Option<String>,

    /// Default user for tag creation
    #[clap(long, global = true)]
    tag_user_name: Option<String>,
}

#[derive(clap::Subcommand, Debug)]
enum Action {
    GenApiSpec,
    Server,
    Benchmark(BenchmarkParameters),
    MultiBenchmark(MultiBenchmarkParameters),
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    //colog::init();

    let config = Config::default()
        .overlay_config(OverlayConfig::read_config_toml(Path::new(
            "/etc/paperless-field-extractor/config.toml",
        )))
        .overlay_config(OverlayConfig::read_from_env())
        .overlay_config(OverlayConfig {
            host: args.host,
            port: args.port,
            paperless_server: args.paperless_server,
            model: args.model,
            num_gpu_layers: args.num_gpu_layers,
            max_ctx: args.max_ctx,
            processing_tag: args.processing_tag,
            processing_color: args.processing_color,
            finished_tag: args.finished_tag,
            finished_color: args.finished_color,
            tag_user_name: args.tag_user_name,
        });


    match args.action {
        Action::GenApiSpec => {
            println!(
                "{}",
                serde_json::to_string_pretty(&server::DocumentProcessingApiSpec::openapi())
                    .unwrap()
            );
            exit(0);
        }
        Action::Benchmark(benchmark_parameters) => {
            benchmark_parameters.run(config).await;
            exit(0);
        }
        Action::MultiBenchmark(multi_benchmark_parameters) => {
            multi_benchmark_parameters.run(config).await;
            exit(0);
        }
        Action::Server => { /* keep going other option stop execution after completion */ }
    }

    let _model_path = Path::new(&config.model)
        .canonicalize()
        .inspect_err(|_| {
            log::error!(
                "Could not find model file! Can not run without a language model! … Stop execution!"
            );
        })
        .unwrap();

    let mut api_client = Client::new_from_env();
    api_client.set_base_url(&config.paperless_server);

    let tags = requests::get_all_tags(&mut api_client).await;
    let users = requests::get_all_users(&mut api_client).await;

    let user = users
        .iter()
        .find(|user| user.username == config.tag_user_name)
        .or_else(|| {
            log::warn!(
                "configured user `{}` could not be found, running without user!",
                config.tag_user_name
            );
            None
        });

    //make sure tags for processing and finshed exists
    let processing_tag = if !tags.iter().any(|t| t.name == config.processing_tag) {
        requests::create_tag(
            &mut api_client,
            user,
            &config.processing_tag,
            &config.processing_color,
        )
        .await
        .inspect_err(|err| {
            log::error!("could not create processing tag: {err}");
        })
        .inspect(|_| {
            log::info!(
                "created processing tag `{}` to paperless ",
                config.processing_tag
            );
        })
        .ok()
    } else {
        tags.iter()
            .find(|t| t.name == config.processing_tag)
            .cloned()
    };

    let finished_tag = if !tags.iter().any(|t| t.name == config.finished_tag) {
        requests::create_tag(
            &mut api_client,
            user,
            &config.finished_tag,
            &config.finished_color,
        )
        .await
        .inspect_err(|err| {
            log::error!("could not create finished tag: {err}");
        })
        .inspect(|_| {
            log::info!(
                "created processing tag `{}` to paperless ",
                config.finished_tag
            );
        })
        .ok()
    } else {
        tags.iter().find(|t| t.name == config.finished_tag).cloned()
    };

    if processing_tag.is_none() || finished_tag.is_none() {
        log::error!("Processing and Finshed Tags could neither be found nor created! Exiting …");
        exit(1);
    }

    let processing_tag = processing_tag.unwrap();
    let finished_tag = finished_tag.unwrap();

    let _ = run_server(config, processing_tag, finished_tag, api_client).await;
}
