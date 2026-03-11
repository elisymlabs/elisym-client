use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;

use crate::cli::error::{CliError, Result};
use crate::constants::{PROTOCOL_FEE_BPS, PROTOCOL_TREASURY};
use crate::skill::{SkillContext, SkillInput, SkillRegistry};
use crate::transport::{IncomingJob, JobFeedbackStatus, Transport};
use crate::tui::AppEvent;

use elisym_core::AgentNode;

pub struct AgentRuntime {
    agent: Arc<AgentNode>,
    skills: SkillRegistry,
    ctx: SkillContext,
    config: RuntimeConfig,
    event_tx: mpsc::UnboundedSender<AppEvent>,
}

pub struct RuntimeConfig {
    pub free_mode: bool,
    pub job_price: u64,
    pub payment_timeout_secs: u32,
    pub max_concurrent_jobs: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            free_mode: false,
            job_price: 10_000_000,
            payment_timeout_secs: 120,
            max_concurrent_jobs: 10,
        }
    }
}

impl AgentRuntime {
    pub fn new(
        agent: Arc<AgentNode>,
        skills: SkillRegistry,
        ctx: SkillContext,
        config: RuntimeConfig,
        event_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        Self {
            agent,
            skills,
            ctx,
            config,
            event_tx,
        }
    }

    pub async fn run(self, transport: Box<dyn Transport>) -> Result<()> {
        let mut jobs_rx = transport.start().await?;

        let transport = Arc::new(transport);
        let skills = Arc::new(self.skills);
        let ctx = Arc::new(self.ctx);
        let agent = self.agent;
        let config = Arc::new(self.config);
        let event_tx = self.event_tx;

        let mut tasks: JoinSet<()> = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_jobs));

        loop {
            tokio::select! {
                Some(job) = jobs_rx.recv() => {
                    let _ = event_tx.send(AppEvent::JobReceived {
                        job_id: job.job_id.clone(),
                        customer_id: job.customer_id.clone(),
                        input: job.input.clone(),
                    });

                    let transport = Arc::clone(&transport);
                    let skills = Arc::clone(&skills);
                    let ctx = Arc::clone(&ctx);
                    let agent = Arc::clone(&agent);
                    let config = Arc::clone(&config);
                    let sem = Arc::clone(&semaphore);
                    let etx = event_tx.clone();

                    tasks.spawn(async move {
                        let _permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(_) => return,
                        };
                        let job_id = job.job_id.clone();
                        if let Err(e) = process_job(&agent, &skills, &ctx, &config, job, transport.as_ref().as_ref(), &etx).await {
                            let _ = etx.send(AppEvent::JobFailed {
                                job_id,
                                error: e.to_string(),
                            });
                        }
                    });
                }
                _ = tokio::signal::ctrl_c() => {
                    break;
                }
                Some(result) = tasks.join_next() => {
                    if let Err(e) = result {
                        let _ = event_tx.send(AppEvent::JobFailed {
                            job_id: String::new(),
                            error: format!("task panicked: {}", e),
                        });
                    }
                }
            }
            while let Some(result) = tasks.try_join_next() {
                if let Err(e) = result {
                    let _ = event_tx.send(AppEvent::JobFailed {
                        job_id: String::new(),
                        error: format!("task panicked: {}", e),
                    });
                }
            }
        }

        // Drain remaining tasks with timeout
        if !tasks.is_empty() {
            let deadline = tokio::time::sleep(Duration::from_secs(30));
            tokio::pin!(deadline);

            loop {
                tokio::select! {
                    Some(_result) = tasks.join_next() => {}
                    _ = &mut deadline => {
                        tasks.abort_all();
                        break;
                    }
                }
                if tasks.is_empty() {
                    break;
                }
            }
        }

        // Drop agent on blocking thread to avoid async drop issues
        match Arc::try_unwrap(agent) {
            Ok(agent) => {
                tokio::task::spawn_blocking(move || drop(agent)).await.ok();
            }
            Err(arc) => {
                tokio::task::spawn_blocking(move || drop(arc)).await.ok();
            }
        }

        Ok(())
    }
}

async fn process_job(
    agent: &AgentNode,
    skills: &SkillRegistry,
    ctx: &SkillContext,
    config: &RuntimeConfig,
    job: IncomingJob,
    transport: &dyn Transport,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<()> {
    let job_id = job.job_id.clone();
    let amount = if config.free_mode {
        None
    } else {
        Some(collect_payment(agent, &job, transport, config.job_price, config.payment_timeout_secs, event_tx).await?)
    };

    // Send Processing feedback
    transport
        .send_feedback(&job, JobFeedbackStatus::Processing)
        .await?;

    // Route to skill and execute
    let skill = skills
        .route(&job.tags)
        .ok_or_else(|| CliError::Other("no skill available to handle this job".into()))?;

    let _ = event_tx.send(AppEvent::SkillStarted {
        job_id: job_id.clone(),
        skill_name: skill.name().to_string(),
    });

    let input = SkillInput {
        data: job.input.clone(),
        input_type: job.input_type.clone(),
        tags: job.tags.clone(),
        metadata: serde_json::Value::Null,
        job_id: job_id.clone(),
    };

    let output = match skill.execute(input, ctx).await {
        Ok(out) => out,
        Err(e) => {
            let _ = event_tx.send(AppEvent::JobFailed {
                job_id: job_id.clone(),
                error: e.to_string(),
            });
            transport
                .send_feedback(
                    &job,
                    JobFeedbackStatus::Error(format!("processing failed: {}", e)),
                )
                .await?;
            return Err(e);
        }
    };

    let result_len = output.data.len();
    transport.deliver_result(&job, &output.data, amount).await?;

    let _ = event_tx.send(AppEvent::JobCompleted {
        job_id,
        result_len,
    });

    // Update wallet balance
    if let Some(solana) = agent.solana_payments() {
        if let Ok(balance) = solana.balance() {
            let _ = event_tx.send(AppEvent::WalletBalance(balance));
        }
    }

    Ok(())
}

async fn collect_payment(
    agent: &AgentNode,
    job: &IncomingJob,
    transport: &dyn Transport,
    price: u64,
    payment_timeout_secs: u32,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<u64> {
    let job_id = job.job_id.clone();
    let payments = agent
        .payments
        .as_ref()
        .ok_or_else(|| CliError::Other("payments not configured".into()))?;

    let fee_amount = (price * PROTOCOL_FEE_BPS).div_ceil(10_000);

    let solana = agent
        .solana_payments()
        .ok_or_else(|| CliError::Other("solana payments not configured".into()))?;

    let payment_request = match solana.create_payment_request_with_fee(
        price,
        &format!("elisym job {}", job.job_id),
        payment_timeout_secs,
        PROTOCOL_TREASURY,
        fee_amount,
    ) {
        Ok(req) => req,
        Err(e) => {
            transport
                .send_feedback(
                    job,
                    JobFeedbackStatus::Error(format!("payment error: {}", e)),
                )
                .await?;
            return Err(e.into());
        }
    };

    let _ = event_tx.send(AppEvent::PaymentRequested {
        job_id: job_id.clone(),
        price,
        fee: fee_amount,
    });

    let chain_str = payment_request.chain.to_string();
    let provider_net = price.saturating_sub(fee_amount);

    // Send PaymentRequired feedback
    transport
        .send_feedback(
            job,
            JobFeedbackStatus::PaymentRequired {
                amount: price,
                payment_request: payment_request.request.clone(),
                chain: chain_str,
            },
        )
        .await?;

    // Poll for payment
    let timeout = Duration::from_secs(payment_timeout_secs as u64);
    let deadline = tokio::time::Instant::now() + timeout;
    let poll_interval = Duration::from_secs(2);

    let paid = loop {
        match payments.lookup_payment(&payment_request.request) {
            Ok(status) if status.settled => break true,
            Ok(_) => {}
            Err(_) => {}
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            break false;
        }
        tokio::time::sleep_until(deadline.min(now + poll_interval)).await;
    };

    if !paid {
        let _ = event_tx.send(AppEvent::PaymentTimeout {
            job_id,
        });
        transport
            .send_feedback(
                job,
                JobFeedbackStatus::Error("payment timeout".into()),
            )
            .await?;
        return Err(CliError::Other("payment timeout".into()));
    }

    let _ = event_tx.send(AppEvent::PaymentReceived {
        job_id,
        net_amount: provider_net,
    });

    Ok(provider_net)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_math() {
        // 0.01 SOL = 10_000_000 lamports, 3% fee = 300_000 lamports
        let price: u64 = 10_000_000;
        let fee = (price * PROTOCOL_FEE_BPS).div_ceil(10_000);
        assert_eq!(fee, 300_000);
        assert_eq!(price.saturating_sub(fee), 9_700_000);
    }

    #[test]
    fn test_fee_math_rounding() {
        // Test rounding up with div_ceil
        let price: u64 = 10_000_001;
        let fee = (price * PROTOCOL_FEE_BPS).div_ceil(10_000);
        // 10_000_001 * 300 = 3_000_000_300, div_ceil(10_000) = 300_001
        assert_eq!(fee, 300_001);
    }
}
