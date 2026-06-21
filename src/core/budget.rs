use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BudgetGuard {
    max_tokens: usize,
    max_iterations: usize,
    max_run_time: Duration,
    used_tokens: usize,
    used_iterations: usize,
    started_at: Instant,
}

impl BudgetGuard {
    pub fn new(max_tokens: usize, max_iterations: usize, max_run_time: Duration) -> Self {
        Self {
            max_tokens,
            max_iterations,
            max_run_time,
            used_tokens: 0,
            used_iterations: 0,
            started_at: Instant::now(),
        }
    }

    pub fn consume_iteration(&mut self) -> Result<(), String> {
        self.used_iterations += 1;
        if self.used_iterations > self.max_iterations {
            return Err("iteration budget exceeded".to_string());
        }
        self.ensure_runtime_budget()
    }

    pub fn consume_tokens(&mut self, count: usize) -> Result<(), String> {
        self.used_tokens += count;
        if self.used_tokens > self.max_tokens {
            return Err("token budget exceeded".to_string());
        }
        Ok(())
    }

    fn ensure_runtime_budget(&self) -> Result<(), String> {
        if self.started_at.elapsed() > self.max_run_time {
            return Err("time budget exceeded".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_budget_is_enforced() {
        let mut guard = BudgetGuard::new(10, 2, Duration::from_secs(1));
        assert!(guard.consume_tokens(8).is_ok());
        assert!(guard.consume_tokens(3).is_err());
    }
}
