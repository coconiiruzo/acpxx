use std::collections::{HashMap, VecDeque};

use tokio::sync::{mpsc, oneshot};

use crate::ProviderId;

#[derive(Clone, Debug)]
pub(crate) struct Scheduler {
    commands: mpsc::UnboundedSender<SchedulerCommand>,
}

#[derive(Debug)]
pub(crate) struct SchedulerPermit {
    provider: ProviderId,
    commands: mpsc::UnboundedSender<SchedulerCommand>,
}

#[derive(Debug)]
enum SchedulerCommand {
    Acquire {
        provider: ProviderId,
        response: oneshot::Sender<SchedulerPermit>,
    },
    Release {
        provider: ProviderId,
    },
}

struct PendingAcquire {
    provider: ProviderId,
    response: oneshot::Sender<SchedulerPermit>,
}

impl Scheduler {
    pub(crate) fn new(global_limit: usize) -> Self {
        Self::with_provider_limits(global_limit, std::iter::empty())
    }

    pub(crate) fn with_provider_limits(
        global_limit: usize,
        provider_limits: impl IntoIterator<Item = (ProviderId, usize)>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let global_limit = global_limit.max(1);
        let provider_limits = provider_limits
            .into_iter()
            .map(|(provider, limit)| (provider, limit.max(1)))
            .collect();
        tokio::spawn(run_scheduler(
            receiver,
            commands.clone(),
            global_limit,
            provider_limits,
        ));
        Self { commands }
    }

    pub(crate) async fn acquire(&self, provider: ProviderId) -> Option<SchedulerPermit> {
        let (response, permit) = oneshot::channel();
        self.commands
            .send(SchedulerCommand::Acquire { provider, response })
            .ok()?;
        permit.await.ok()
    }
}

impl Drop for SchedulerPermit {
    fn drop(&mut self) {
        let _ = self.commands.send(SchedulerCommand::Release {
            provider: self.provider,
        });
    }
}

async fn run_scheduler(
    mut commands: mpsc::UnboundedReceiver<SchedulerCommand>,
    command_sender: mpsc::UnboundedSender<SchedulerCommand>,
    global_limit: usize,
    provider_limits: HashMap<ProviderId, usize>,
) {
    let mut pending = VecDeque::<PendingAcquire>::new();
    let mut active_total = 0_usize;
    let mut active_by_provider = HashMap::<ProviderId, usize>::new();

    while let Some(command) = commands.recv().await {
        match command {
            SchedulerCommand::Acquire { provider, response } => {
                pending.push_back(PendingAcquire { provider, response });
            }
            SchedulerCommand::Release { provider } => {
                active_total = active_total.saturating_sub(1);
                let active = active_by_provider.entry(provider).or_default();
                *active = active.saturating_sub(1);
            }
        }

        loop {
            while pending
                .front()
                .is_some_and(|request| request.response.is_closed())
            {
                pending.pop_front();
            }
            let Some(request) = pending.front() else {
                break;
            };
            let provider_limit = provider_limits
                .get(&request.provider)
                .copied()
                .unwrap_or(global_limit);
            let provider_active = active_by_provider
                .get(&request.provider)
                .copied()
                .unwrap_or(0);
            if active_total == global_limit || provider_active == provider_limit {
                break;
            }

            let request = pending.pop_front().expect("front request must exist");
            active_total += 1;
            *active_by_provider.entry(request.provider).or_default() += 1;
            let _ = request.response.send(SchedulerPermit {
                provider: request.provider,
                commands: command_sender.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn admission_is_strict_fifo() {
        let scheduler = Scheduler::new(1);
        let first = scheduler.acquire(ProviderId::Grok).await.unwrap();
        let second_scheduler = scheduler.clone();
        let second =
            tokio::spawn(async move { second_scheduler.acquire(ProviderId::Grok).await.unwrap() });
        tokio::task::yield_now().await;
        let third_scheduler = scheduler.clone();
        let mut third =
            tokio::spawn(async move { third_scheduler.acquire(ProviderId::Cursor).await.unwrap() });
        tokio::task::yield_now().await;

        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(1), second)
            .await
            .expect("second request must be admitted first")
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut third)
                .await
                .is_err(),
            "third request bypassed the FIFO head"
        );
        drop(second);
        tokio::time::timeout(Duration::from_secs(1), third)
            .await
            .expect("third request must be admitted after release")
            .unwrap();
    }

    #[tokio::test]
    async fn provider_limit_is_enforced_with_global_capacity_remaining() {
        let scheduler = Scheduler::with_provider_limits(2, [(ProviderId::Grok, 1)]);
        let grok = scheduler.acquire(ProviderId::Grok).await.unwrap();
        let cursor = scheduler.acquire(ProviderId::Cursor).await.unwrap();
        drop(cursor);

        let next_scheduler = scheduler.clone();
        let mut next_grok =
            tokio::spawn(async move { next_scheduler.acquire(ProviderId::Grok).await.unwrap() });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut next_grok)
                .await
                .is_err(),
            "provider limit admitted a second Grok Run"
        );
        drop(grok);
        tokio::time::timeout(Duration::from_secs(1), next_grok)
            .await
            .expect("provider capacity must be released with its permit")
            .unwrap();
    }
}
