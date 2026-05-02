use rand::Rng;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

pub struct CircuitBreaker {
    failure_threshold: u32,
    recovery_timeout: Duration,
    failure_count: u32,
    last_failure_time: Option<Instant>,
    state: CircuitState,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            failure_threshold,
            recovery_timeout,
            failure_count: 0,
            last_failure_time: None,
            state: CircuitState::Closed,
        }
    }

    pub fn allow_request(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last_fail) = self.last_failure_time {
                    if last_fail.elapsed() >= self.recovery_timeout {
                        self.state = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
            CircuitState::HalfOpen => true,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitState::Closed;
        self.last_failure_time = None;
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure_time = Some(Instant::now());
        if self.failure_count >= self.failure_threshold {
            self.state = CircuitState::Open;
        }
    }

    pub fn state(&self) -> CircuitState {
        self.state
    }
}

pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.3,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            ..Default::default()
        }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let exponential = self.base_delay * 2u32.pow(attempt.saturating_sub(1));
        let capped = exponential.min(self.max_delay);

        let mut rng = rand::thread_rng();
        let jitter_range = (capped.as_millis() as f64 * self.jitter_factor) as i64;
        let jitter = if jitter_range > 0 {
            rng.gen_range(-jitter_range..=jitter_range)
        } else {
            0
        };

        let delay_ms = capped.as_millis() as i64 + jitter;
        if delay_ms < 0 {
            Duration::ZERO
        } else {
            Duration::from_millis(delay_ms as u64)
        }
    }
}

pub fn should_retry_http_status(status: u16) -> bool {
    matches!(
        status,
        429 | 500 | 502 | 503 | 504 | 408
    )
}

pub fn should_retry_error(error: &reqwest::Error) -> bool {
    if error.is_timeout() || error.is_connect() || error.is_request() {
        return true;
    }
    if let Some(status) = error.status() {
        return should_retry_http_status(status.as_u16());
    }
    false
}
