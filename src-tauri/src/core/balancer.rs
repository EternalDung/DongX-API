use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Circuit breaker state for a channel
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    pub failure_count: u32,
    pub is_open: bool,
    pub last_failure_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            failure_count: 0,
            is_open: false,
            last_failure_at: None,
        }
    }

    /// Record a failure; open circuit if threshold exceeded
    pub fn record_failure(&mut self, threshold: u32) {
        self.failure_count += 1;
        self.last_failure_at = Some(chrono::Utc::now());
        if self.failure_count >= threshold {
            self.is_open = true;
        }
    }

    /// Record a success; reset circuit
    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.is_open = false;
        self.last_failure_at = None;
    }

    /// Check if circuit is open and cooldown has passed (half-open state)
    pub fn should_try_probe(&self, cooldown_secs: i64) -> bool {
        if !self.is_open {
            return true;
        }
        if let Some(last) = self.last_failure_at {
            let elapsed = chrono::Utc::now().signed_duration_since(last);
            return elapsed.num_seconds() >= cooldown_secs;
        }
        false
    }
}

/// Load balancer: manages circuit breakers per channel
pub struct LoadBalancer {
    breakers: Arc<RwLock<HashMap<String, CircuitBreaker>>>,
    failure_threshold: u32,
    cooldown_secs: i64,
}

impl LoadBalancer {
    pub fn new(failure_threshold: u32, cooldown_secs: i64) -> Self {
        Self {
            breakers: Arc::new(RwLock::new(HashMap::new())),
            failure_threshold,
            cooldown_secs,
        }
    }

    /// Check if a channel is available (not circuit-broken or in cooldown)
    pub async fn is_available(&self, channel_id: &str) -> bool {
        let breakers = self.breakers.read().await;
        if let Some(cb) = breakers.get(channel_id) {
            return cb.should_try_probe(self.cooldown_secs);
        }
        true
    }

    /// Record a failure for a channel
    pub async fn record_failure(&self, channel_id: &str) {
        let mut breakers = self.breakers.write().await;
        let cb = breakers.entry(channel_id.to_string()).or_insert_with(CircuitBreaker::new);
        cb.record_failure(self.failure_threshold);
    }

    /// Record a success for a channel
    pub async fn record_success(&self, channel_id: &str) {
        let mut breakers = self.breakers.write().await;
        let cb = breakers.entry(channel_id.to_string()).or_insert_with(CircuitBreaker::new);
        cb.record_success();
    }

    /// Weighted random selection among available channels
    /// channels: Vec<(channel_id, weight)>
    pub async fn weighted_select(&self, channels: &[(String, i32)]) -> Option<String> {
        let available: Vec<_> = {
            let breakers = self.breakers.read().await;
            channels
                .iter()
                .filter(|(id, _)| {
                    if let Some(cb) = breakers.get(id) {
                        cb.should_try_probe(self.cooldown_secs)
                    } else {
                        true
                    }
                })
                .collect()
        };

        if available.is_empty() {
            return None;
        }

        let total_weight: i32 = available.iter().map(|(_, w)| (*w).max(1)).sum();
        if total_weight == 0 {
            return Some(available[0].0.clone());
        }

        let mut rng = rand::thread_rng();
        let mut r = rand::Rng::gen_range(&mut rng, 0..total_weight);
        for (id, w) in &available {
            r -= (*w).max(1);
            if r < 0 {
                return Some(id.clone());
            }
        }
        Some(available[0].0.clone())
    }
}
