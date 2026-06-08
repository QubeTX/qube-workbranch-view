//! Coalesce a burst of raw filesystem signals into a single refresh request,
//! firing once the storm has been quiet for `delay`.

use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedReceiver};

/// Read raw signals from `raw` and emit one `()` on `refresh` per quiet period.
/// Returns when either channel closes.
pub async fn run(mut raw: UnboundedReceiver<()>, refresh: Sender<()>, delay: Duration) {
    while raw.recv().await.is_some() {
        // Keep resetting the timer while events keep arriving.
        loop {
            tokio::select! {
                () = tokio::time::sleep(delay) => break,
                again = raw.recv() => {
                    if again.is_none() {
                        return;
                    }
                }
            }
        }
        if refresh.send(()).await.is_err() {
            return;
        }
    }
}
