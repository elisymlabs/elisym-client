use serde::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;

/// Heartbeat message for ping/pong liveness checks (NIP-17 encrypted).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub nonce: String,
}

impl HeartbeatMessage {
    pub fn ping(nonce: String) -> Self {
        Self {
            msg_type: "elisym_ping".into(),
            nonce,
        }
    }

    pub fn pong(nonce: String) -> Self {
        Self {
            msg_type: "elisym_pong".into(),
            nonce,
        }
    }

    pub fn is_ping(&self) -> bool {
        self.msg_type == "elisym_ping"
    }

    pub fn is_pong(&self) -> bool {
        self.msg_type == "elisym_pong"
    }
}

/// Generate a cryptographically random nonce using OS entropy (via Solana's OsRng).
pub fn random_nonce() -> String {
    bs58::encode(&Keypair::new().to_bytes()[..16]).into_string()
}
