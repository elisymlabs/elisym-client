use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::error::{CliError, Result};

/// Validate that an agent name is safe for use as a directory name.
/// Allows ASCII alphanumeric characters, hyphens, and underscores only.
pub fn validate_agent_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CliError::Other("agent name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(CliError::Other("invalid agent name".into()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CliError::Other(
            "agent name may only contain letters, digits, hyphens, and underscores".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub relays: Vec<String>,
    pub secret_key: String,
    pub payment: PaymentSection,
    #[serde(default)]
    pub inactive_capabilities: Vec<String>,
    #[serde(default)]
    pub capability_prompts: HashMap<String, String>,
    #[serde(default)]
    pub llm: Option<LlmSection>,
    #[serde(default)]
    pub customer_llm: Option<LlmSection>,
    #[serde(default)]
    pub encryption: Option<super::crypto::EncryptionSection>,
}

impl AgentConfig {
    /// Returns true if secrets are encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.encryption.is_some()
    }

    /// Bundle all secret fields for encryption.
    pub fn secrets_bundle(&self) -> super::crypto::SecretsBundle {
        super::crypto::SecretsBundle {
            nostr_secret_key: self.secret_key.clone(),
            solana_secret_key: self.payment.solana_secret_key.clone(),
            llm_api_key: self.llm.as_ref().map(|l| l.api_key.clone()).unwrap_or_default(),
            customer_llm_api_key: self.customer_llm.as_ref().map(|l| l.api_key.clone()),
        }
    }

    /// Encrypt secrets with password and clear plaintext fields in-place.
    pub fn encrypt_secrets(&mut self, password: &str) -> Result<()> {
        let bundle = self.secrets_bundle();
        let section = super::crypto::encrypt_secrets(&bundle, password)?;
        self.encryption = Some(section);
        self.secret_key = String::new();
        self.payment.solana_secret_key = String::new();
        if let Some(ref mut llm) = self.llm {
            llm.api_key = String::new();
        }
        if let Some(ref mut cllm) = self.customer_llm {
            cllm.api_key = String::new();
        }
        Ok(())
    }

    /// Decrypt secrets with password and populate plaintext fields in-place.
    pub fn decrypt_secrets(&mut self, password: &str) -> Result<()> {
        let section = self.encryption.as_ref()
            .ok_or_else(|| CliError::Other("config is not encrypted".into()))?;
        let bundle = super::crypto::decrypt_secrets(section, password)?;
        self.secret_key = bundle.nostr_secret_key;
        self.payment.solana_secret_key = bundle.solana_secret_key;
        if let Some(ref mut llm) = self.llm {
            llm.api_key = bundle.llm_api_key;
        }
        if let Some(ref mut cllm) = self.customer_llm {
            if let Some(key) = bundle.customer_llm_api_key {
                cllm.api_key = key;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmSection {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    4096
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentSection {
    pub chain: String,
    pub network: String,
    #[serde(default)]
    pub rpc_url: Option<String>,
    pub job_price: u64,
    pub payment_timeout_secs: u32,
    pub solana_secret_key: String,
}

impl Default for PaymentSection {
    fn default() -> Self {
        Self {
            chain: "solana".to_string(),
            network: "mainnet".to_string(),
            rpc_url: None,
            job_price: 10_000_000, // 0.01 SOL in lamports
            payment_timeout_secs: 120,
            solana_secret_key: String::new(),
        }
    }
}

impl PaymentSection {
    /// Derive the Solana public address from the stored secret key for display.
    pub fn solana_address(&self) -> Option<String> {
        let bytes = bs58::decode(&self.solana_secret_key).into_vec().ok()?;
        let keypair = solana_sdk::signature::Keypair::try_from(bytes.as_slice()).ok()?;
        Some(solana_sdk::signer::Signer::pubkey(&keypair).to_string())
    }
}

/// Root directory: ~/.elisym/agents/
fn agents_root() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| CliError::Other("cannot find home directory".into()))?;
    Ok(home.join(".elisym").join("agents"))
}

/// Directory for a specific agent: ~/.elisym/agents/<name>/
pub fn agent_dir(name: &str) -> Result<PathBuf> {
    validate_agent_name(name)?;
    Ok(agents_root()?.join(name))
}

/// Path to config.toml for a specific agent
pub fn config_path(name: &str) -> Result<PathBuf> {
    Ok(agent_dir(name)?.join("config.toml"))
}

/// Save agent config to disk, creating directories as needed
pub fn save_config(config: &AgentConfig) -> Result<()> {
    let dir = agent_dir(&config.name)?;
    fs::create_dir_all(&dir)?;

    let toml_str = toml::to_string_pretty(config)?;
    fs::write(config_path(&config.name)?, toml_str)?;
    Ok(())
}

/// Load agent config from disk
pub fn load_config(name: &str) -> Result<AgentConfig> {
    let path = config_path(name)?;
    let contents = fs::read_to_string(&path).map_err(|e| {
        CliError::Other(format!("agent '{}' not found ({})", name, e))
    })?;
    let config: AgentConfig = toml::from_str(&contents)?;
    Ok(config)
}

/// List all configured agent names
pub fn list_agents() -> Result<Vec<String>> {
    let root = agents_root()?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                let cfg = entry.path().join("config.toml");
                if cfg.exists() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

/// Delete an agent directory entirely
pub fn delete_agent(name: &str) -> Result<()> {
    let dir = agent_dir(name)?;
    if !dir.exists() {
        return Err(CliError::Other(format!("agent '{}' not found", name)));
    }
    fs::remove_dir_all(dir)?;
    Ok(())
}
