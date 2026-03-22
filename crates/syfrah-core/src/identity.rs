use serde::{Deserialize, Serialize};

/// Identity of a node in the mesh.
/// Contains the WireGuard public key (the private key is stored separately)
/// and a human-readable name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIdentity {
    /// Human-readable node name
    pub name: String,
    /// WireGuard public key (base64-encoded x25519)
    pub wg_public_key: String,
}

impl NodeIdentity {
    pub fn new(name: String, wg_public_key: String) -> Self {
        Self { name, wg_public_key }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let identity = NodeIdentity::new(
            "node-1".into(),
            "dGVzdC1wdWJsaWMta2V5".into(),
        );
        let json = serde_json::to_string(&identity).unwrap();
        let parsed: NodeIdentity = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "node-1");
        assert_eq!(parsed.wg_public_key, identity.wg_public_key);
    }
}
