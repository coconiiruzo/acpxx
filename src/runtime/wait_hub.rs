use std::sync::Arc;

use tokio::sync::watch;

use crate::runtime::RunObservation;
use crate::{ControlError, Result, RunReceipt, WaitOptions};

pub async fn wait_for_terminal(
    mut receiver: watch::Receiver<RunObservation>,
    options: WaitOptions,
) -> Result<Arc<RunReceipt>> {
    let wait = async {
        loop {
            if let RunObservation::Terminal(receipt) = &*receiver.borrow_and_update() {
                return Ok(receipt.clone());
            }
            receiver
                .changed()
                .await
                .map_err(|_| ControlError::ActorClosed)?;
        }
    };

    match options.timeout {
        Some(timeout) => tokio::time::timeout(timeout, wait)
            .await
            .map_err(|_| ControlError::WaitTimeout { timeout })?,
        None => wait.await,
    }
}
