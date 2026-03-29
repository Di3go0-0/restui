use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    cancelled: Arc<AtomicBool>,
    task: Option<std::thread::JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_clone = cancelled.clone();

        // Use a dedicated OS thread instead of tokio::spawn so that the
        // blocking event::poll() call does not hold up the async runtime
        // and can be joined reliably on shutdown.
        let task = std::thread::spawn(move || {
            loop {
                if cancelled_clone.load(Ordering::Relaxed) {
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
            cancelled,
            task: Some(task),
        }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }

    /// Stop the event polling thread and wait for it to finish so that
    /// terminal cleanup can proceed without a zombie process.
    pub fn stop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() {
            // The thread will exit within one tick_rate cycle (250ms)
            let _ = task.join();
        }
    }
}
