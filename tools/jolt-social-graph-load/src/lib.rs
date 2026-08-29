use std::collections::BTreeMap;

use anyhow::{bail, Context};
use jolt_identity::NodeIdentity;
use serde::{Deserialize, Serialize};

pub mod runtime;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkloadConfig {
    pub seed: u64,
    pub daemon_count: usize,
    pub identity_count: usize,
    pub follow_count: usize,
    pub records_per_identity: usize,
    pub churn_percent: u8,
}

impl WorkloadConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.daemon_count < 2 {
            bail!("daemon_count must include one reader and at least one provider");
        }
        if self.identity_count == 0 {
            bail!("identity_count must be at least one");
        }
        if self.follow_count == 0 || self.follow_count > self.identity_count {
            bail!("follow_count must be between one and identity_count");
        }
        if self.records_per_identity == 0 {
            bail!("records_per_identity must be at least one");
        }
        if self.churn_percent > 100 {
            bail!("churn_percent cannot exceed 100");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordPlan {
    pub index: usize,
    pub path: String,
    pub body: String,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct AuthorPlan {
    pub index: usize,
    pub identity: String,
    pub signing_key_hex: String,
    pub provider_index: usize,
    pub records: Vec<RecordPlan>,
}

impl AuthorPlan {
    pub fn identity_key(&self) -> anyhow::Result<NodeIdentity> {
        let bytes = hex::decode(&self.signing_key_hex).context("invalid planned signing key")?;
        let signing_key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("planned signing key must contain 32 bytes"))?;
        NodeIdentity::from_signing_key_bytes(&signing_key)
            .map_err(|error| anyhow::anyhow!("invalid planned identity: {error}"))
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkloadPlan {
    pub seed: u64,
    pub provider_count: usize,
    pub authors: Vec<AuthorPlan>,
    pub followed_authors: Vec<usize>,
    pub churned_providers: Vec<usize>,
}

impl WorkloadPlan {
    pub fn generate(config: &WorkloadConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let provider_count = config.daemon_count - 1;
        let mut authors = Vec::with_capacity(config.identity_count);

        for index in 0..config.identity_count {
            let signing_key = deterministic_bytes(config.seed, "identity", index);
            let identity = NodeIdentity::from_signing_key_bytes(&signing_key)
                .map_err(|error| anyhow::anyhow!("failed to derive identity {index}: {error}"))?;
            let records = (0..config.records_per_identity)
                .map(|record_index| RecordPlan {
                    index: record_index,
                    path: format!("/spoke/posts/author-{index:05}-record-{record_index:05}"),
                    body: serde_json::json!({
                        "authorIndex": index,
                        "recordIndex": record_index,
                        "seed": config.seed,
                        "text": format!("deterministic post {record_index} from author {index}"),
                    })
                    .to_string(),
                })
                .collect();
            authors.push(AuthorPlan {
                index,
                identity: identity.identity_id().to_string(),
                signing_key_hex: hex::encode(signing_key),
                provider_index: index % provider_count,
                records,
            });
        }

        let followed_authors = select_indices(
            config.seed,
            "follow",
            config.identity_count,
            config.follow_count,
        );
        let churn_count = if config.churn_percent == 0 {
            0
        } else {
            (provider_count * usize::from(config.churn_percent)).div_ceil(100)
        };
        let churned_providers = select_indices(
            config.seed,
            "churn",
            provider_count,
            churn_count.min(provider_count),
        );

        Ok(Self {
            seed: config.seed,
            provider_count,
            authors,
            followed_authors,
            churned_providers,
        })
    }

    pub fn record_count(&self) -> usize {
        self.authors.iter().map(|author| author.records.len()).sum()
    }
}

pub(crate) fn deterministic_bytes(seed: u64, domain: &str, index: usize) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"jolt-social-graph-load-v1\0");
    hasher.update(&seed.to_le_bytes());
    hasher.update(domain.as_bytes());
    hasher.update(&index.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn select_indices(seed: u64, domain: &str, count: usize, take: usize) -> Vec<usize> {
    let mut ranked: Vec<_> = (0..count)
        .map(|index| (deterministic_bytes(seed, domain, index), index))
        .collect();
    ranked.sort_unstable();
    ranked.truncate(take);
    ranked.into_iter().map(|(_, index)| index).collect()
}

#[derive(Debug, Clone, Default)]
pub struct PhaseAccounting {
    successful_latencies_micros: Vec<u64>,
    successes: u64,
    failures: BTreeMap<String, u64>,
}

impl PhaseAccounting {
    pub fn record_success(&mut self, latency_micros: u64) {
        self.successful_latencies_micros.push(latency_micros);
        self.successes += 1;
    }

    pub fn record_failure(&mut self, _latency_micros: u64, reason: impl Into<String>) {
        *self.failures.entry(reason.into()).or_default() += 1;
    }

    pub fn summarize(&self) -> PhaseSummary {
        let mut latencies = self.successful_latencies_micros.clone();
        latencies.sort_unstable();
        PhaseSummary {
            operations: self.successes + self.failures.values().sum::<u64>(),
            successes: self.successes,
            failures: self.failures.clone(),
            latency_micros: LatencyPercentiles {
                min: latencies.first().copied().unwrap_or_default(),
                p50: percentile(&latencies, 50),
                p95: percentile(&latencies, 95),
                p99: percentile(&latencies, 99),
                max: latencies.last().copied().unwrap_or_default(),
            },
        }
    }
}

fn percentile(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatencyPercentiles {
    pub min: u64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PhaseSummary {
    pub operations: u64,
    pub successes: u64,
    pub failures: BTreeMap<String, u64>,
    pub latency_micros: LatencyPercentiles,
}

#[cfg(test)]
mod tests {
    use super::{PhaseAccounting, WorkloadConfig, WorkloadPlan};

    #[test]
    fn fixed_seed_reproduces_workload_and_result_accounting() {
        let config = WorkloadConfig {
            seed: 42,
            daemon_count: 4,
            identity_count: 12,
            follow_count: 8,
            records_per_identity: 3,
            churn_percent: 25,
        };

        let first = WorkloadPlan::generate(&config).unwrap();
        let second = WorkloadPlan::generate(&config).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.followed_authors.len(), 8);
        assert_eq!(first.authors.len(), 12);
        assert_eq!(first.record_count(), 36);

        let mut first_accounting = PhaseAccounting::default();
        first_accounting.record_success(1_000);
        first_accounting.record_success(4_000);
        first_accounting.record_success(2_000);
        first_accounting.record_failure(99_000, "timeout");
        let first_summary = first_accounting.summarize();

        let mut second_accounting = PhaseAccounting::default();
        second_accounting.record_success(1_000);
        second_accounting.record_success(4_000);
        second_accounting.record_success(2_000);
        second_accounting.record_failure(99_000, "timeout");
        let second_summary = second_accounting.summarize();

        assert_eq!(first_summary, second_summary);
        assert_eq!(first_summary.operations, 4);
        assert_eq!(first_summary.successes, 3);
        assert_eq!(first_summary.failures["timeout"], 1);
        assert_eq!(first_summary.latency_micros.p50, 2_000);
        assert_eq!(first_summary.latency_micros.p95, 4_000);
        assert_eq!(first_summary.latency_micros.max, 4_000);
    }
}
