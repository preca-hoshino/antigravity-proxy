use super::storage::{AccountStorage, AntigravityAccount};
use crate::config::AppConfig;
use crate::upstream::headers::generate_fingerprint;
use crate::upstream::oauth::refresh_access_token;
use crate::upstream::quota::fetch_quota;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AccountManager {
    accounts: Arc<RwLock<Vec<AntigravityAccount>>>,
    storage: Arc<AccountStorage>,
    #[allow(dead_code)]
    config: Arc<AppConfig>,
    client: Client,
}

impl AccountManager {
    pub fn new(storage: AccountStorage, config: Arc<AppConfig>, client: Client) -> Self {
        let loaded = storage.load_accounts();
        Self {
            accounts: Arc::new(RwLock::new(loaded)),
            storage: Arc::new(storage),
            config,
            client,
        }
    }

    pub async fn get_accounts(&self) -> Vec<AntigravityAccount> {
        self.accounts.read().await.clone()
    }

    pub async fn add_or_update_account(&self, mut account: AntigravityAccount) -> Result<(), String> {
        if account.fingerprint.is_none() {
            account.fingerprint = Some(generate_fingerprint(Some(&account.email)));
        }

        let mut lock = self.accounts.write().await;
        if let Some(pos) = lock.iter().position(|a| a.email == account.email) {
            lock[pos] = account;
        } else {
            lock.push(account);
        }

        self.storage.save_accounts(&lock)
    }

    pub async fn remove_account(&self, email: &str) -> Result<bool, String> {
        let mut lock = self.accounts.write().await;
        let prev_len = lock.len();
        lock.retain(|a| a.email != email);
        let removed = lock.len() < prev_len;

        if removed {
            self.storage.save_accounts(&lock)?;
        }
        Ok(removed)
    }

    pub async fn mark_cooldown(&self, email: &str, pool: &str, duration_ms: u64) {
        let key = format!("{}|{}", email, pool);
        let expiry = chrono::Utc::now().timestamp_millis() + duration_ms as i64;

        let mut lock = self.accounts.write().await;
        if let Some(acc) = lock.iter_mut().find(|a| a.email == email) {
            acc.cooldowns.insert(key, expiry);
            acc.health_score = (acc.health_score - 10.0).max(0.0);
            let _ = self.storage.save_accounts(&lock);
        }
    }

    pub async fn reset_cooldown(&self, email: &str) {
        let mut lock = self.accounts.write().await;
        if let Some(acc) = lock.iter_mut().find(|a| a.email == email) {
            acc.cooldowns.clear();
            acc.health_score = 100.0;
            let _ = self.storage.save_accounts(&lock);
        }
    }

    pub async fn reset_all_cooldowns(&self) {
        let mut lock = self.accounts.write().await;
        for acc in lock.iter_mut() {
            acc.cooldowns.clear();
            acc.health_score = 100.0;
        }
        let _ = self.storage.save_accounts(&lock);
    }

    pub async fn update_project_id(&self, email: &str, project_id: &str) -> Result<bool, String> {
        let mut lock = self.accounts.write().await;
        if let Some(acc) = lock.iter_mut().find(|a| a.email == email) {
            acc.project_id = Some(project_id.to_string());
            self.storage.save_accounts(&lock)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn get_best_account(
        &self,
        pool: &str,
        _model: &str,
        tried_emails: &[String],
    ) -> Option<AntigravityAccount> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut candidates = Vec::new();

        {
            let accounts = self.accounts.read().await;
            for acc in accounts.iter() {
                if tried_emails.contains(&acc.email) {
                    continue;
                }

                let key = format!("{}|{}", acc.email, pool);
                if let Some(&cooldown_expiry) = acc.cooldowns.get(&key) {
                    if cooldown_expiry > now {
                        continue;
                    }
                }

                candidates.push(acc.clone());
            }
        }

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            let score_a = a.health_score - (a.last_used as f64 * 0.00001);
            let score_b = b.health_score - (b.last_used as f64 * 0.00001);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut chosen = candidates.into_iter().next()?;

        let buffer_ms = 60_000;
        let needs_refresh = match chosen.expires_at {
            Some(exp) => now + buffer_ms >= exp,
            None => true,
        };

        if needs_refresh || chosen.access_token.is_none() {
            tracing::info!(email = %chosen.email, "Refreshing access token before upstream request");
            match refresh_access_token(&self.client, &chosen.refresh_token).await {
                Ok(token_resp) => {
                    chosen.access_token = Some(token_resp.access_token);
                    chosen.expires_at = Some(now + (token_resp.expires_in * 1000));

                    let mut lock = self.accounts.write().await;
                    if let Some(pos) = lock.iter().position(|a| a.email == chosen.email) {
                        lock[pos].access_token = chosen.access_token.clone();
                        lock[pos].expires_at = chosen.expires_at;
                        let _ = self.storage.save_accounts(&lock);
                    }
                }
                Err(e) => {
                    tracing::error!(email = %chosen.email, error = %e, "Failed to refresh access token");
                    return None;
                }
            }
        }

        if chosen.fingerprint.is_none() {
            chosen.fingerprint = Some(generate_fingerprint(Some(&chosen.email)));
            let mut lock = self.accounts.write().await;
            if let Some(pos) = lock.iter().position(|a| a.email == chosen.email) {
                lock[pos].fingerprint = chosen.fingerprint.clone();
                let _ = self.storage.save_accounts(&lock);
            }
        }

        {
            let mut lock = self.accounts.write().await;
            if let Some(pos) = lock.iter().position(|a| a.email == chosen.email) {
                lock[pos].last_used = now;
                let _ = self.storage.save_accounts(&lock);
            }
        }

        Some(chosen)
    }

    pub async fn refresh_all_quotas(&self) {
        let accounts = self.accounts.read().await.clone();

        for acc in accounts {
            let fp = acc.fingerprint.clone().unwrap_or_else(|| generate_fingerprint(Some(&acc.email)));
            if let (Some(token), Some(project_id)) = (&acc.access_token, &acc.project_id) {
                if let Ok(quotas) = fetch_quota(&self.client, token, &fp, project_id).await {
                    let mut lock = self.accounts.write().await;
                    if let Some(pos) = lock.iter().position(|a| a.email == acc.email) {
                        lock[pos].quota = Some(quotas);
                        let _ = self.storage.save_accounts(&lock);
                    }
                }
            }
        }
    }
}
