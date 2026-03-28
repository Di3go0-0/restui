use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    cancel: CancellationToken,
    _task: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let task = tokio::spawn(async move {
            loop {
                if cancel_clone.is_cancelled() {
                    break;
                }
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                            if tx.send(AppEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Resize(w, h)) => {
                            if tx.send(AppEvent::Resize(w, h)).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                } else if tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        Self {
            rx,
            cancel,
            _task: task,
        }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }

    /// Stop the event polling task so terminal cleanup can proceed
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}
