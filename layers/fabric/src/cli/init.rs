use crate::daemon::{self, DaemonConfig};
use anyhow::Result;
use std::net::SocketAddr;

pub async fn run(
    name: &str,
    node_name: &str,
    port: u16,
    endpoint: Option<SocketAddr>,
    peering_port: u16,
) -> Result<()> {
    daemon::run_init(DaemonConfig {
        mesh_name: name.to_string(),
        node_name: node_name.to_string(),
        wg_listen_port: port,
        public_endpoint: endpoint,
        peering_port,
    })
    .await
}
