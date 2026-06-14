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
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::token::data::LlamaTokenData;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use schemars::Schema;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::num::NonZeroU32;
use std::path::Path;
use thiserror::Error;

use gbnf::{self, GrammarItem, NonTerminalSymbol, ProductionItem, RepetitionType, TerminalSymbol};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenGenerationStats {
    pub prompt_tokens: usize,
    pub prompt_elapsed_ms: f64,
    pub injected_tokens: u64,
    pub injected_elapsed_ms: f64,
    pub sampled_tokens: u64,
    pub sampled_elapsed_ms: f64,
    pub forward_passes: u64,
}

impl TokenGenerationStats {
    pub fn new() -> Self {
        Self {
            prompt_tokens: 0,
            prompt_elapsed_ms: 0.0,
            injected_tokens: 0,
            injected_elapsed_ms: 0.0,
            sampled_tokens: 0,
            sampled_elapsed_ms: 0.0,
            forward_passes: 0,
        }
    }

    pub fn add(&mut self, other: &Self) {
        self.prompt_tokens += other.prompt_tokens;
        self.prompt_elapsed_ms += other.prompt_elapsed_ms;
        self.injected_tokens += other.injected_tokens;
        self.injected_elapsed_ms += other.injected_elapsed_ms;
        self.sampled_tokens += other.sampled_tokens;
        self.sampled_elapsed_ms += other.sampled_elapsed_ms;
        self.forward_passes += other.forward_passes;
    }

    pub fn prompt_tps(&self) -> f64 {
        if self.prompt_elapsed_ms == 0.0 {
            return 0.0;
        }
        self.prompt_tokens as f64 / (self.prompt_elapsed_ms / 1_000.0)
    }

    pub fn injected_tps(&self) -> f64 {
        if self.injected_elapsed_ms == 0.0 {
            return 0.0;
        }
        self.injected_tokens as f64 / (self.injected_elapsed_ms / 1_000.0)
    }

    pub fn sampled_tps(&self) -> f64 {
        if self.sampled_elapsed_ms == 0.0 {
            return 0.0;
        }
        self.sampled_tokens as f64 / (self.sampled_elapsed_ms / 1_000.0)
    }

    pub fn overall_tps(&self) -> f64 {
        let total_ms = self.injected_elapsed_ms + self.sampled_elapsed_ms;
        if total_ms == 0.0 {
            return 0.0;
        }
        (self.injected_tokens + self.sampled_tokens) as f64 / (total_ms / 1_000.0)
    }
}

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

fn build_first_char_index(model: &LlamaModel) -> FirstCharIndex {
    let vocab_size = model.n_vocab();
    let mut map: HashMap<char, Vec<LlamaToken>> = HashMap::new();
    let mut reps: Vec<LlamaToken> = Vec::new();
    let mut seen_chars: std::collections::HashSet<char> = std::collections::HashSet::new();

    for i in 0..vocab_size {
        let tok = LlamaToken(i);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let piece = match model.token_to_piece(tok, &mut decoder, true, None) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let first = piece.chars().next();
        if let Some(c) = first {
            if seen_chars.insert(c) {
                reps.push(model.str_to_token(&c.to_string(), AddBos::Never).unwrap()[0]);
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
            let mut decoder = encoding_rs::UTF_8.new_decoder();
            let piece = model
                .token_to_piece(td.id(), &mut decoder, true, None)
                .ok()?;
            Some((td.id(), piece.to_string()))
        })
        .collect();
    if token_pieces.len() != valid_tokens.len() {
        // prefix matching can only fail if some possible next tokens can not be decoded!
        return None;
    }
    token_pieces.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let longest = token_pieces.first()?.0.clone();
    let longest_str = &token_pieces.first()?.1;
    let is_prefix_chain = token_pieces
        .iter()
        .skip(1)
        .all(|(_, s)| longest_str.starts_with(s));
    if is_prefix_chain { Some(longest) } else { None }
}

/// First-char probing: use a tiny array of representative tokens (one per
/// first Unicode char) to ask the grammar sampler which first chars are
/// valid.  If exactly one first char survives, build a filtered array of
/// all tokens with that first char and apply grammar again.
fn try_grammar_based_deterministic_inject(
    grammar_sampler: &LlamaSampler,
    index: &FirstCharIndex,
    model: &LlamaModel,
) -> Option<LlamaToken> {
    if index.representatives.is_empty() {
        return None;
    }

    let rep_data: Vec<LlamaTokenData> = index
        .representatives
        .iter()
        .map(|&t| LlamaTokenData::new(t, 0.0, 0.0))
        .collect();
    let mut rep_array = LlamaTokenDataArray::new(rep_data, false);

    grammar_sampler.apply(&mut rep_array);

    let valid_reps: Vec<LlamaToken> = rep_array
        .data
        .iter()
        .filter(|td| !td.logit().is_infinite())
        .map(|td| td.id())
        .collect();

    let n_valid = valid_reps.len();

    if n_valid == 0 {
        return None;
    }

    if n_valid > 1 {
        return None;
    }

    let winning_rep = valid_reps[0];

    let winning_char = index
        .map
        .iter()
        .find_map(|(&c, tokens)| tokens.iter().any(|t| *t == winning_rep).then_some(c))?;

    let candidate_tokens = index.map.get(&winning_char)?;

    if candidate_tokens.is_empty() {
        return None;
    }

    if candidate_tokens.len() == 1 {
        return Some(candidate_tokens[0]);
    }

    let narrowed_data: Vec<LlamaTokenData> = candidate_tokens
        .iter()
        .map(|&t| LlamaTokenData::new(t, 0.0, 0.0))
        .collect();
    let mut narrowed_array = LlamaTokenDataArray::new(narrowed_data, false);

    grammar_sampler.apply(&mut narrowed_array);

    let valid = collect_valid_tokens(&narrowed_array);
    find_longest_prefix_token(&valid, model)
}

#[derive(Debug, Error)]
pub enum ModelError {
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

pub struct LLModelExtractor {
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

        let first_char_index = build_first_char_index(&model);

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
        let mut grammar_sampler = LlamaSampler::grammar(&self.model, &grammar, "root").unwrap();
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

        let mut forward_passes = 0u64;

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
            forward_passes += 1;
        }
        batch.clear();

        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut n_cur = tokens_list.len() as i32;
        let mut output = String::new();
        let mut injected_tokens = 0u64;
        let mut sampled_tokens = 0u64;

        let mut last_inject_substring: Option<String> = None;
        while n_cur as usize <= n_len {
            // Fast path: probe with first-char index (~50-500 tokens)
            if let Some(injected) = try_grammar_based_deterministic_inject(
                &grammar_sampler,
                &self.first_char_index,
                &self.model,
            ) {
                grammar_sampler.accept(injected);
                sampler.accept(injected);
                injected_tokens += 1;

                if injected == self.model.token_eos() {
                    break;
                }

                let output_string = self
                    .model
                    .token_to_piece(injected, &mut decoder, true, None)
                    .unwrap();
                if dry_run {
                    print!("{output_string}");
                    let _ = std::io::stdout().flush();
                }
                output.push_str(&output_string);
                if let Some(last_inject_substring) = &mut last_inject_substring {
                    last_inject_substring.push_str(&output_string);
                } else {
                    last_inject_substring = Some(output_string)
                }
            } else {
                if let Some(last_inject_substring) = last_inject_substring.take() {
                    let tokens_list = self
                        .model
                        .str_to_token(&last_inject_substring, AddBos::Never)
                        .unwrap_or_else(|_| panic!("failed to tokenize injection"));
                    let last_index = tokens_list.len() as i32 - 1;
                    for (batch_i, token_batch) in tokens_list.chunks(batch_chunk_size).enumerate() {
                        batch.clear();
                        for (i, token) in (0_usize..).zip(token_batch.into_iter()) {
                            let is_last = (batch_i * batch_chunk_size + i) == last_index as usize;
                            batch.add(
                                *token,
                                n_cur,
                                &[0],
                                is_last,
                            )?;
                            n_cur += 1;
                        }
                        ctx.decode(&mut batch)?;
                        forward_passes += 1;
                    }
                    batch.clear();
                }

                // Check EOS from last decode
                if output.ends_with(&self.eos_string) {
                    break;
                }

                // Sample a non-deterministic token
                sampled_tokens += 1;
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                grammar_sampler.accept(token);

                if token == self.model.token_eos() {
                    break;
                }
                batch.add(token, n_cur, &[0], true)?;
                n_cur += 1;
                ctx.decode(&mut batch).expect("failed to eval");
                forward_passes += 1;
                batch.clear();

                let output_string = self
                    .model
                    .token_to_piece(token, &mut decoder, true, None)
                    .unwrap();
                if dry_run {
                    print!("{output_string}");
                    let _ = std::io::stdout().flush();
                }
                output.push_str(&output_string);
            }
        }
        log::debug!(
            "extraction stats: forward_passes={} injected={} sampled={}",
            forward_passes, injected_tokens, sampled_tokens
        );
        // remove eos token
        let output = output.replace(&self.eos_string, "");
        //println!("{output}");
        Ok(serde_json::from_str(&output)?)
    }
}
