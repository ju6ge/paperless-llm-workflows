use llama_cpp_2::DecodeError;
use llama_cpp_2::LlamaCppError;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::BatchAddError;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::AddBos;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use llama_cpp_2::token::LlamaToken;
use schemars::Schema;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::Path;
use thiserror::Error;

use gbnf::{self, GrammarItem, NonTerminalSymbol, ProductionItem, RepetitionType, TerminalSymbol};

fn gen_gbnf(schema: &schemars::Schema, eos_token: String) -> String {
    let js = &serde_json::to_string(schema.as_value()).unwrap();
    let mut gram = gbnf::Grammar::from_json_schema(js)
        .map_err(|err| {
            println!("{err}");
            err
        })
        .unwrap();
    for mut r in gram.items.iter_mut() {
        match &mut r {
            GrammarItem::LineBreak | GrammarItem::Comment(_) => {}
            GrammarItem::Rule(rule) => {
                if rule.lhs.name == "root"
                    && let Some(last_rule) = rule.rhs.items.last_mut()
                {
                    *last_rule = gbnf::ProductionItem::Terminal(
                        TerminalSymbol {
                            value: eos_token.clone(),
                        },
                        gbnf::RepetitionType::One,
                    );
                }
            }
        }
    }
    if let Some(p) = gram.recurring_items.get_mut(&NonTerminalSymbol {
        name: "ws".to_string(),
    }) && let Some(last_item) = p.items.last_mut()
        && let ProductionItem::CharacterSet(_, rep_type) = last_item
    {
        *rep_type = RepetitionType::One
    }
    gram.to_string()
}

fn collect_valid_tokens(data_array: &LlamaTokenDataArray) -> Vec<LlamaTokenData> {
    data_array
        .data
        .iter()
        .copied()
        .filter(|td| !td.logit().is_infinite())
        .collect()
}

/// Maps each possible first Unicode code point to the list of vocabulary
/// tokens that start with it.  Built once per model load.
pub struct FirstCharIndex {
    /// first Unicode char → tokens whose decoded piece starts with that char
    map: HashMap<char, Vec<LlamaToken>>,
    /// one representative token per first char, used for lightweight probing
    representatives: Vec<LlamaToken>,
}

fn build_first_char_index(
    model: &LlamaModel,
    decoder: &mut encoding_rs::Decoder,
) -> FirstCharIndex {
    let vocab_size = model.n_vocab();
    let mut map: HashMap<char, Vec<LlamaToken>> = HashMap::new();
    let mut reps: Vec<LlamaToken> = Vec::new();
    let mut seen_chars: std::collections::HashSet<char> = std::collections::HashSet::new();

    for i in 0..vocab_size {
        let tok = LlamaToken(i);
        let piece = match model.token_to_piece(tok, decoder, true, None) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let first = piece.chars().next();
        if let Some(c) = first {
            if seen_chars.insert(c) {
                reps.push(tok);
            }
            map.entry(c).or_default().push(tok);
        }
    }

    FirstCharIndex {
        map,
        representatives: reps,
    }
}

fn find_longest_prefix_token(
    valid_tokens: &[LlamaTokenData],
    model: &LlamaModel,
    decoder: &mut encoding_rs::Decoder,
) -> Option<llama_cpp_2::token::LlamaToken> {
    let count = valid_tokens.len();
    if count == 0 {
        return None;
    }
    if count == 1 {
        return Some(valid_tokens[0].id());
    }
    if count > 20 {
        return None;
    }
    // Multiple valid tokens: check if they form a prefix chain
    let mut token_pieces: Vec<(llama_cpp_2::token::LlamaToken, String)> = valid_tokens
        .iter()
        .filter_map(|td| {
            let piece = model.token_to_piece(td.id(), decoder, true, None).ok()?;
            Some((td.id(), piece.to_string()))
        })
        .collect();
    token_pieces.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let longest = token_pieces.first()?.0.clone();
    let longest_str = &token_pieces.first()?.1;
    let is_prefix_chain = token_pieces
        .iter()
        .skip(1)
        .all(|(_, s)| longest_str.starts_with(s));
    if is_prefix_chain { Some(longest) } else { None }
}

#[derive(Debug, Error)]
pub(crate) enum ModelError {
    #[error(transparent)]
    FormatDeserializationError(#[from] serde_json::Error),
    #[error("Model has not been loaded!")]
    ModelNotLoaded,
    #[error(transparent)]
    LlamaCppError(#[from] LlamaCppError),
    #[error(transparent)]
    LlamaDecodeError(#[from] DecodeError),
    #[error(transparent)]
    LlamaBatchAddError(#[from] BatchAddError),
}

pub(crate) struct LLModelExtractor {
    backend: LlamaBackend,
    model: LlamaModel,
    ctx_params: LlamaContextParams,
    eos_string: String,
    first_char_index: FirstCharIndex,
}

impl LLModelExtractor {
    pub fn new(
        model_path: &Path,
        num_gpu_layers: usize,
        ctx_size_max: Option<u32>,
    ) -> Result<Self, ModelError> {
        let mut backend = LlamaBackend::init()?;
        backend.void_logs();
        let params = LlamaModelParams::default().with_n_gpu_layers(num_gpu_layers as u32);
        let model = LlamaModel::load_from_file(&backend, model_path, &params)
            .expect("unable to load model");

        let ctx_size = ctx_size_max
            .map(|s| std::cmp::min(s, model.n_ctx_train()))
            .unwrap_or(model.n_ctx_train());

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(ctx_size).unwrap()))
            .with_n_batch(ctx_size);

        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let eos_string = &model
            .token_to_piece(model.token_eos(), &mut decoder, true, None)
            .unwrap()
            .to_string();

        let first_char_index = build_first_char_index(&model, &mut decoder);

        Ok(Self {
            backend,
            model,
            ctx_params,
            eos_string: eos_string.to_string(),
            first_char_index,
        })
    }

    pub fn extract(
        &mut self,
        base_data: &Value,
        response_schema: &Schema,
        dry_run: bool,
    ) -> Result<Value, ModelError> {
        let grammar = gen_gbnf(response_schema, self.eos_string.to_string());
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::grammar(&self.model, &grammar, "root").unwrap(),
            LlamaSampler::dry(&self.model, 5., 1.75, 2, 256, ["\"", ":", "*"]),
            LlamaSampler::min_p(0.01, 64),
            LlamaSampler::temp(0.1),
            LlamaSampler::dist(rand::random()),
        ]);
        let prompt = format!("{}\n", serde_json::to_string(base_data).unwrap());
        let mut ctx = self
            .model
            .new_context(&self.backend, self.ctx_params.clone())
            .expect("unable to create the llama_context");
        let tokens_list = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .unwrap_or_else(|_| panic!("failed to tokenize {prompt}"));
        let n_len = tokens_list.len() + 4096;

        let batch_chunk_size: usize = 512;
        // create a llama_batch with size 512
        // we use this object to submit token data for decoding
        let mut batch = LlamaBatch::new(batch_chunk_size, 1);

        let last_index = tokens_list.len() as i32 - 1;
        for (batch_i, token_batch) in tokens_list.chunks(batch_chunk_size).enumerate() {
            batch.clear();
            for (i, token) in (0_usize..).zip(token_batch.into_iter()) {
                // llama_decode will output logits only for the last token of the prompt
                let is_last = (batch_i * batch_chunk_size + i) == last_index as usize;
                batch.add(
                    *token,
                    (batch_i * batch_chunk_size + i) as i32,
                    &[0],
                    is_last,
                )?;
            }
            ctx.decode(&mut batch)?;
        }
        batch.clear();

        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut n_cur = tokens_list.len() as i32;
        let mut output = String::new();
        while n_cur as usize <= n_len {
            // sample the next token
            {
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);

                // grammar sampling alread accepts this so calling accept again leads to segfaults
                //sampler.accept(token);

                // is it an end of stream?
                if token == self.model.token_eos() || output.ends_with(&self.eos_string) {
                    break;
                }

                let output_string = self
                    .model
                    .token_to_piece(token, &mut decoder, true, None)
                    .unwrap();
                if dry_run {
                    print!("{output_string}");
                    if n_cur % 100 == 0 {
                        let _ = std::io::stdout().flush();
                    }
                }
                output.push_str(&output_string);

                batch.clear();
                batch.add(token, n_cur, &[0], true)?;
            }

            n_cur += 1;

            ctx.decode(&mut batch).expect("failed to eval");
        }
        // remove eos token
        let output = output.replace(&self.eos_string, "");
        //println!("{output}");
        Ok(serde_json::from_str(&output)?)
    }
}
