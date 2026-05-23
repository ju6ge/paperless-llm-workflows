use std::{cmp::max, collections::BTreeMap, iter::zip, sync::Arc};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::{FutureExt, StreamExt, select};
use futures_timer::Delay;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Gauge, Paragraph, Row, Table},
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Duration;

use crate::benchmark::{BenchmarkResults, ProgressUpdate};
use crate::extract::TokenGenerationStats;

#[derive(Debug, Clone)]
enum BenchmarkState {
    Pending,
    Running,
    Finished,
}

#[derive(Debug, Clone)]
struct BenchmarkRunData {
    state: BenchmarkState,
    result: Option<BenchmarkResults>,
    finished_docs: usize,
    total_docs: usize,
    latest_token_stats: Option<TokenGenerationStats>,
}

impl BenchmarkRunData {
    fn new(total_docs: usize) -> Self {
        Self {
            state: BenchmarkState::Pending,
            result: None,
            finished_docs: 0,
            total_docs,
            latest_token_stats: None,
        }
    }
}

pub(crate) struct BenchmarkApp {
    running: Arc<tokio::sync::RwLock<bool>>,
    benchmarking_update_channel: Mutex<tokio::sync::mpsc::UnboundedReceiver<ProgressUpdate>>,
    event_stream: Mutex<EventStream>,
    benchmark_progress: tokio::sync::RwLock<BTreeMap<String, BenchmarkRunData>>,
    log_messages: tokio::sync::RwLock<Vec<ProgressUpdate>>,
}

impl BenchmarkApp {
    pub fn new(
        shared_running_flag: Arc<tokio::sync::RwLock<bool>>,
        update_channel: tokio::sync::mpsc::UnboundedReceiver<ProgressUpdate>,
    ) -> Self {
        Self {
            running: shared_running_flag,
            benchmarking_update_channel: Mutex::new(update_channel),
            event_stream: Default::default(),
            benchmark_progress: RwLock::new(BTreeMap::new()),
            log_messages: RwLock::new(Vec::new()),
        }
    }

    /// update app state based on benchmarking results to display current status
    async fn handle_benchmarking_updates(&self) {
        let mut update_channel = self.benchmarking_update_channel.lock().await;
        if let Ok(process_update) = (*update_channel).try_recv() {
            match &process_update {
                ProgressUpdate::Register {
                    model_name,
                    total_docs,
                } => {
                    let mut progress = self.benchmark_progress.write().await;
                    (*progress).insert(model_name.to_string(), BenchmarkRunData::new(*total_docs));
                }
                ProgressUpdate::Started { model_name, .. } => {
                    let mut progress = self.benchmark_progress.write().await;
                    if let Some(benchmark_data) = progress.get_mut(model_name) {
                        benchmark_data.state = BenchmarkState::Running
                    }
                }
                ProgressUpdate::DocumentProgress {
                    model_name,
                    progress,
                    ..
                } => {
                    let mut progress_data = self.benchmark_progress.write().await;
                    if let Some(benchmark_data) = progress_data.get_mut(model_name) {
                        benchmark_data.finished_docs = *progress;
                        if benchmark_data.finished_docs == benchmark_data.total_docs {
                            benchmark_data.state = BenchmarkState::Finished;
                        }
                    }
                }
                ProgressUpdate::BenchmarkResults {
                    model_name,
                    results,
                } => {
                    let mut progress_data = self.benchmark_progress.write().await;
                    if let Some(benchmark_data) = progress_data.get_mut(model_name) {
                        benchmark_data.result = Some(results.clone())
                    }
                }
                ProgressUpdate::TokenStats { model_name, doc_token_stats } => {
                    let mut progress_data = self.benchmark_progress.write().await;
                    if let Some(benchmark_data) = progress_data.get_mut(model_name) {
                        benchmark_data.latest_token_stats = Some(doc_token_stats.clone());
                    }
                }
                _ => { /* nothing to do here */ }
            }
            let mut log_messages = self.log_messages.write().await;
            log_messages.push(process_update);
        }
    }

    /// Run the application's main loop.
    pub async fn run(self, mut terminal: DefaultTerminal) -> Result<(), std::io::Error> {
        while *self.running.read().await {
            self.handle_benchmarking_updates().await;
            let log = self.log_messages.read().await.clone();
            let benchmark_state = (*self.benchmark_progress.read().await).clone();
            terminal.draw(|frame| self.draw(frame, &log, &benchmark_state))?;
            self.handle_crossterm_events().await?;
        }
        Ok(())
    }

    /// Renders the user interface.
    ///
    /// This is where you add new widgets. See the following resources for more information:
    /// - <https://docs.rs/ratatui/latest/ratatui/widgets/index.html>
    /// - <https://github.com/ratatui/ratatui/tree/master/examples>
    fn draw(
        &self,
        frame: &mut Frame,
        log_messages: &Vec<ProgressUpdate>,
        benchmark_state: &BTreeMap<String, BenchmarkRunData>,
    ) {
        use Constraint::{Fill, Length, Min};
        let title = Line::from("Paperless LLM Workflows Benchmark Status")
            .bold()
            .blue()
            .centered();
        let text = "\n\
            \n\
            Press `Esc`, `Ctrl-C` or `q` to stop running.";

        let vertical = Layout::vertical([Length(6), Min(0)]);
        let [title_area, content_area] = vertical.areas(frame.area());
        let horizontal = Layout::horizontal([Fill(1); 2]);
        let [info_area, log_area] = horizontal.areas(content_area);
        let benchmark_size: u16 = ((benchmark_state.keys().len()) * 3 + 2) as u16;
        let token_perf_size: u16 = (benchmark_state.keys().len() * 2 + 3) as u16;
        let infos = Layout::vertical([Length(benchmark_size), Length(token_perf_size), Min(0)]);
        let [progress_area, token_perf_area, result_area] = infos.areas(info_area);
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::bordered().title(title))
                .centered(),
            title_area,
        );
        render_logs(frame, log_messages, log_area);
        render_progess_bars(frame, benchmark_state, progress_area);
        render_token_performance(frame, benchmark_state, token_perf_area);
        render_results(frame, benchmark_state, result_area);
    }

    /// Reads the crossterm events and updates the state of [`App`].
    async fn handle_crossterm_events(&self) -> Result<(), std::io::Error> {
        let mut event_stream = self.event_stream.lock().await;
        let mut event = event_stream.next().fuse();
        let mut delay = Delay::new(Duration::from_millis(500)).fuse();
        select! {
            _ = delay => {}
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(evt)) => match evt {
                        Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key).await,
                        Event::Mouse(_) => {}
                        Event::Resize(_, _) => {}
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    async fn on_key_event(&self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit().await,
            // Add other key handlers here.
            _ => {}
        }
    }

    /// Set running to false to quit the application.
    async fn quit(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
}

fn render_logs(frame: &mut Frame, logs: &Vec<ProgressUpdate>, area: Rect) {
    let logs_block = Block::bordered().title("LOG ");
    let logs_inner = logs_block.inner(area);
    let height = logs_inner.as_size().height;
    let skip = max(logs.len() as i64 - height as i64, 0);
    let log_text = logs
        .iter()
        .skip(skip as usize)
        .map(|m| match m {
            ProgressUpdate::Register { model_name, .. } => {
                Line::raw(format!("Pending Benchmark for {model_name}"))
                    .style(Style::new().magenta())
            }
            ProgressUpdate::Started { model_name, .. } => {
                Line::raw(format!("Started Benchmark for {model_name}"))
                    .style(Style::new().light_blue())
            }
            ProgressUpdate::DocumentProgress { model_name, .. } => {
                Line::raw(format!("Progress update for {model_name}")).style(Style::new().yellow())
            }
            ProgressUpdate::BenchmarkResults { model_name, .. } => {
                Line::raw(format!("Results updated for {model_name}")).style(Style::new().green())
            }
            ProgressUpdate::Error { model_name, .. } => {
                Line::raw(format!("Error for {model_name}")).style(Style::new().red())
            }
            ProgressUpdate::TokenStats { model_name, .. } => {
                Line::raw(format!("Token stats for {model_name}")).style(Style::new().yellow())
            }
            ProgressUpdate::Finished { model_name } => {
                Line::raw(format!("Benchmark finihed {model_name}")).style(Style::new().cyan())
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(logs_block, area);
    frame.render_widget(Paragraph::new(log_text), logs_inner);
}

fn render_results(
    frame: &mut Frame,
    benchmark_state: &BTreeMap<String, BenchmarkRunData>,
    area: Rect,
) {
    use Constraint::Fill;

    let result_block = Block::bordered().title("Result Preview");
    let result_inner = result_block.inner(area);
    let mut rows = vec![];
    for (model_name, data) in benchmark_state.iter() {
        if let Some(results) = &data.result {
            let (succeded, failed, errored, success_rate) = results.current_stats();
            rows.push(Row::new(vec![
                format!("{}", model_name),
                format!("{}", succeded),
                format!("{}", failed),
                format!("{}", errored),
                format!("{:.2} %", success_rate * 100.),
            ]));
        }
    }
    let widths = [Fill(3), Fill(1), Fill(1), Fill(1), Fill(1)];
    let table = Table::new(rows, widths).header(Row::new(vec![
        "Model",
        "Success",
        "Failed",
        "Errors",
        "Sucess-Rate",
    ]));

    frame.render_widget(result_block, area);
    frame.render_widget(table, result_inner);
}

fn render_token_performance(
    frame: &mut Frame,
    benchmark_state: &BTreeMap<String, BenchmarkRunData>,
    area: Rect,
) {
    let token_block = Block::bordered().title("Token Performance (t/s)");
    let token_inner = token_block.inner(area);
    let mut rows = vec![];
    for (model_name, data) in benchmark_state.iter() {
        if let Some(stats) = &data.latest_token_stats {
            rows.push(Row::new(vec![
                model_name.clone(),
                format!("{:.1}", stats.prompt_tps()),
                format!("{:.1}", stats.injected_tps()),
                format!("{:.1}", stats.sampled_tps()),
                format!("{:.1}", stats.overall_tps()),
            ]));
        }
    }
    let widths = [Constraint::Fill(3), Constraint::Fill(1), Constraint::Fill(1), Constraint::Fill(1), Constraint::Fill(1)];
    let table = Table::new(rows, widths).header(Row::new(vec![
        "Model",
        "Prompt",
        "Inject",
        "Sample",
        "Overall",
    ]));

    frame.render_widget(&token_block, area);
    frame.render_widget(table, token_inner);
}

fn render_progess_bars(
    frame: &mut Frame,
    benchmark_state: &BTreeMap<String, BenchmarkRunData>,
    area: Rect,
) {
    use Constraint::Length;
    let progress_block = Block::bordered().title("Benchmark Progress");
    let progress_inner = progress_block.inner(area);
    let amount_bars = if benchmark_state.keys().len() > 1 {
        benchmark_state.keys().len() + 1
    } else {
        1
    };
    let bar_layout =
        Layout::vertical(vec![Length(2); amount_bars]).flex(ratatui::layout::Flex::Center);
    let bar_areas = bar_layout.split(progress_inner);
    frame.render_widget(progress_block, area);
    if benchmark_state.keys().len() > 1 {
        let total_docs: usize = benchmark_state.values().map(|d| d.total_docs).sum();
        let finished_docs: usize = benchmark_state.values().map(|d| d.finished_docs).sum();
        frame.render_widget(
            Gauge::default()
                .block(Block::new().title("Overall"))
                .label(format!(" {}/{}", finished_docs, total_docs))
                .ratio(finished_docs as f64 / total_docs as f64),
            bar_areas[0],
        );
        for ((model_name, data), area) in zip(benchmark_state.iter(), bar_areas[1..].iter()) {
            frame.render_widget(
                Gauge::default()
                    .block(Block::new().title(model_name.as_str()))
                    .label(format!(" {}/{}", data.finished_docs, data.total_docs))
                    .ratio(data.finished_docs as f64 / data.total_docs as f64),
                *area,
            );
        }
    } else {
        for ((model_name, data), area) in zip(benchmark_state.iter(), bar_areas.iter()) {
            frame.render_widget(
                Gauge::default()
                    .block(Block::new().title(model_name.as_str()))
                    .label(format!(" {}/{}", data.finished_docs, data.total_docs))
                    .ratio(data.finished_docs as f64 / data.total_docs as f64),
                *area,
            );
        }
    }
}
