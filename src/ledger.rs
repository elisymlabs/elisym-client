use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cli::config::agent_dir;
use crate::cli::error::{CliError, Result};

/// Persistent status of a job in the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerStatus {
    /// Payment confirmed, skill execution not yet started or in progress.
    Paid,
    /// Skill executed successfully, result ready but not yet delivered.
    Executed,
    /// Result delivered to customer.
    Delivered,
    /// Job failed after payment (skill error or delivery error).
    Failed,
}

/// A single job entry persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub job_id: String,
    pub status: LedgerStatus,
    pub input: String,
    pub input_type: String,
    pub tags: Vec<String>,
    pub customer_id: String,
    pub bid: Option<u64>,
    /// The payment request string (for lookup_payment verification).
    pub payment_request: String,
    /// Net amount received by provider (after protocol fee).
    pub net_amount: u64,
    /// Cached skill output — stored after successful execution so delivery can be retried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// The raw Nostr event JSON — needed to call submit_job_result on recovery.
    pub raw_event_json: String,
    /// Unix timestamp when the entry was created.
    pub created_at: u64,
    /// Number of recovery attempts.
    #[serde(default)]
    pub retry_count: u32,
}

/// Persistent job ledger backed by a JSON file.
///
/// Tracks jobs that have been paid so they can be recovered if the agent
/// crashes or execution/delivery fails.
pub struct JobLedger {
    path: PathBuf,
    entries: HashMap<String, LedgerEntry>,
}

impl JobLedger {
    /// Open (or create) the ledger for the given agent.
    pub fn open(agent_name: &str) -> Result<Self> {
        let dir = agent_dir(agent_name)?;
        let path = dir.join("jobs.json");

        let entries = if path.exists() {
            let data = fs::read_to_string(&path)?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self { path, entries })
    }

    /// Persist current state to disk.
    fn flush(&self) -> Result<()> {
        let data = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| CliError::Other(format!("ledger serialize: {}", e)))?;
        fs::write(&self.path, data)?;
        Ok(())
    }

    /// Record a paid job. Called right after payment confirmation.
    #[allow(clippy::too_many_arguments)]
    pub fn record_paid(
        &mut self,
        job_id: &str,
        input: &str,
        input_type: &str,
        tags: &[String],
        customer_id: &str,
        bid: Option<u64>,
        payment_request: &str,
        net_amount: u64,
        raw_event_json: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = LedgerEntry {
            job_id: job_id.to_string(),
            status: LedgerStatus::Paid,
            input: input.to_string(),
            input_type: input_type.to_string(),
            tags: tags.to_vec(),
            customer_id: customer_id.to_string(),
            bid,
            payment_request: payment_request.to_string(),
            net_amount,
            result: None,
            raw_event_json: raw_event_json.to_string(),
            created_at: now,
            retry_count: 0,
        };

        self.entries.insert(job_id.to_string(), entry);
        self.flush()
    }

    /// Mark job as executed with cached result.
    pub fn mark_executed(&mut self, job_id: &str, result: &str) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(job_id) {
            entry.status = LedgerStatus::Executed;
            entry.result = Some(result.to_string());
            self.flush()?;
        }
        Ok(())
    }

    /// Mark job as delivered — final state.
    pub fn mark_delivered(&mut self, job_id: &str) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(job_id) {
            entry.status = LedgerStatus::Delivered;
            entry.result = None; // free memory, no longer needed
            self.flush()?;
        }
        Ok(())
    }

    /// Mark job as permanently failed.
    pub fn mark_failed(&mut self, job_id: &str) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(job_id) {
            entry.status = LedgerStatus::Failed;
            self.flush()?;
        }
        Ok(())
    }

    /// Reset a failed job back to Paid status for manual retry.
    /// Resets retry_count to 0 so it gets a fresh set of attempts.
    pub fn reset_for_retry(&mut self, job_id: &str) -> Result<bool> {
        if let Some(entry) = self.entries.get_mut(job_id) {
            if entry.status == LedgerStatus::Failed {
                entry.status = if entry.result.is_some() {
                    LedgerStatus::Executed
                } else {
                    LedgerStatus::Paid
                };
                entry.retry_count = 0;
                self.flush()?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Increment retry count for a job.
    pub fn increment_retry(&mut self, job_id: &str) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(job_id) {
            entry.retry_count += 1;
            self.flush()?;
        }
        Ok(())
    }

    /// Get all entries (for TUI display).
    /// Sorted: pending first (Paid, Executed, Failed), then Delivered at the bottom.
    /// Within each group, newest first.
    pub fn all_entries(&self) -> Vec<LedgerEntry> {
        let mut entries: Vec<_> = self.entries.values().cloned().collect();
        entries.sort_by(|a, b| {
            let rank = |s: &LedgerStatus| match s {
                LedgerStatus::Paid => 0,
                LedgerStatus::Executed => 1,
                LedgerStatus::Failed => 2,
                LedgerStatus::Delivered => 3,
            };
            rank(&a.status).cmp(&rank(&b.status))
                .then_with(|| b.created_at.cmp(&a.created_at))
        });
        entries
    }

    /// Get all jobs that need recovery (paid or executed but not delivered).
    pub fn pending_jobs(&self) -> Vec<&LedgerEntry> {
        self.entries
            .values()
            .filter(|e| e.status == LedgerStatus::Paid || e.status == LedgerStatus::Executed)
            .collect()
    }

    /// Clean up old delivered/failed entries older than `max_age_secs`.
    pub fn gc(&mut self, max_age_secs: u64) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let to_remove: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                (e.status == LedgerStatus::Delivered || e.status == LedgerStatus::Failed)
                    && now.saturating_sub(e.created_at) > max_age_secs
            })
            .map(|(k, _)| k.clone())
            .collect();

        if !to_remove.is_empty() {
            for key in &to_remove {
                self.entries.remove(key);
            }
            self.flush()?;
        }

        Ok(())
    }
}
