use std::net::SocketAddr;
use anyhow::Result;
use syfrah_net::daemon::{self, DaemonConfig};

pub async fn run(secret: &str, node_name: &str, port: u16, endpoint: Option<SocketAddr>, ipfs_api: Option<String>) -> Result<()> {
    daemon::run_join(secret, DaemonConfig {
        mesh_name: "mesh".to_string(),
        node_name: node_name.to_string(),
        wg_listen_port: port,
        public_endpoint: endpoint,
        ipfs_api,
    }).await
}
