//! Split raw filesystem events into two lanes:
//!
//! - an **activity** lane that forwards the changed paths *immediately* (so the
//!   live blue save-marker feels instant), lossy by design — if the UI is
//!   briefly behind, a dropped pulse is harmless, the next save re-lights it;
//! - a **refresh** lane that coalesces a burst into a single re-snapshot once
//!   the storm has been quiet for `delay` (the Git capture is expensive).

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc::{Sender, UnboundedReceiver};

/// Read raw path-batches from `raw`; mirror each to `activity` at once and emit
/// one `()` on `refresh` per quiet period. Returns when the raw channel closes.
pub async fn run(
    mut raw: UnboundedReceiver<Vec<PathBuf>>,
    activity: Sender<Vec<PathBuf>>,
    refresh: Sender<()>,
    delay: Duration,
) {
    while let Some(paths) = raw.recv().await {
        let _ = activity.try_send(paths);
        // Keep resetting the timer while events keep arriving.
        loop {
            tokio::select! {
                () = tokio::time::sleep(delay) => break,
                again = raw.recv() => match again {
                    Some(paths) => {
                        let _ = activity.try_send(paths);
                    }
                    None => return,
                }
            }
        }
        if refresh.send(()).await.is_err() {
            return;
        }
    }
}
