use std::{collections::BTreeMap, sync::Arc};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::{select, FutureExt, StreamExt};
use futures_timer::Delay;
use itertools::Itertools;
use ratatui::{
    layout::{Layout, Constraint}, style::Stylize, text::Line, widgets::{Block, Paragraph}, DefaultTerminal, Frame,
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration};

use crate::benchmark::ProgressUpdate;

#[derive(Debug)]
struct BenchmarkState {
    
}

pub(crate) struct BenchmarkApp {
    running: Arc<tokio::sync::RwLock<bool>>,
    benchmarking_update_channel: Mutex<tokio::sync::mpsc::UnboundedReceiver<ProgressUpdate>>,
    event_stream: Mutex<EventStream>,
    benchmark_progress: tokio::sync::RwLock<BTreeMap<String, BenchmarkState>>,
    log_messages: tokio::sync::RwLock<Vec<ProgressUpdate>>,
}

impl BenchmarkApp {
    pub fn new(shared_running_flag: Arc<tokio::sync::RwLock<bool>>, update_channel: tokio::sync::mpsc::UnboundedReceiver<ProgressUpdate>) -> Self {
        Self {
            running: shared_running_flag,
            benchmarking_update_channel: Mutex::new(update_channel),
            event_stream: Default::default(),
            benchmark_progress: RwLock::new(BTreeMap::new()),
            log_messages: RwLock::new(Vec::new())
        }
    }

    /// update app state based on benchmarking results to display current status
    async fn handle_benchmarking_updates(&self) {
        let mut update_channel = self.benchmarking_update_channel.lock().await;
        if let Ok(process_update) = (*update_channel).try_recv() {
            let mut log_messages = self.log_messages.write().await;
            log_messages.push(process_update);
        }
    }

    /// Run the application's main loop.
    pub async fn run(self, mut terminal: DefaultTerminal) -> color_eyre::Result<()> {
        while *self.running.read().await {
            self.handle_benchmarking_updates().await;
            let log = self.log_messages.read().await.clone();
            terminal.draw(|frame| self.draw(frame, &log))?;
            self.handle_crossterm_events().await?;
        }
        Ok(())
    }

    /// Renders the user interface.
    ///
    /// This is where you add new widgets. See the following resources for more information:
    /// - <https://docs.rs/ratatui/latest/ratatui/widgets/index.html>
    /// - <https://github.com/ratatui/ratatui/tree/master/examples>
    fn draw(&self, frame: &mut Frame, log_messages: &Vec<ProgressUpdate>) {
        use Constraint::{Fill, Length, Min};
        let title = Line::from("Paperless LLM Workflows Benchmark Status")
            .bold()
            .blue()
            .centered();
        let text = "\n\
            \n\
            Press `Esc`, `Ctrl-C` or `q` to stop running.";
        let log = format!("LOG:\n{}", log_messages.iter().map(|m| format!("{:?}",m)).join("\n-"));

        let vertical = Layout::vertical([Length(6), Min(0)]);
        let [ title_area, content_area ] = vertical.areas(frame.area());
        let horizontal = Layout::horizontal([Fill(1); 2]);
        let [ info_area, log_area ] = horizontal.areas(content_area);
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::bordered().title(title))
                .centered(),
            title_area,
        );
        frame.render_widget(
            Paragraph::new(log)
                .block(Block::bordered())
                .centered(),
            log_area,
        );
    }

    /// Reads the crossterm events and updates the state of [`App`].
    async fn handle_crossterm_events(&self) -> color_eyre::Result<()> {
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
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => {
                self.quit().await
            },
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
