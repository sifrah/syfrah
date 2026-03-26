//! Query module for topology-aware peer lookups.
//!
//! [`TopologyView`] indexes peers by [`Region`] and [`Zone`] at construction
//! time (O(n)), then answers every query in O(1) via `HashMap` lookups.

use std::collections::HashMap;

use syfrah_core::mesh::{PeerRecord, PeerStatus, Region, Zone};

use crate::store::StoreError;

/// Default region used when a peer has no topology information.
const DEFAULT_REGION: &str = "default";
/// Default zone used when a peer has no topology information.
const DEFAULT_ZONE: &str = "default";

/// Pre-indexed view of mesh peers grouped by region and zone.
///
/// Construction is O(n) over the peer list; all subsequent queries are O(1)
/// hash-map lookups (amortised).
pub struct TopologyView {
    by_region: HashMap<Region, Vec<PeerRecord>>,
    by_zone: HashMap<Zone, Vec<PeerRecord>>,
    zone_to_region: HashMap<Zone, Region>,
}

impl TopologyView {
    /// Build a [`TopologyView`] from the on-disk store.
    ///
    /// Loads all peers via [`crate::store::load`] and indexes them.
    pub fn snapshot() -> Result<Self, StoreError> {
        let state = crate::store::load()?;
        Ok(Self::from_peers(&state.peers))
    }

    /// Build a [`TopologyView`] from an arbitrary peer slice.
    ///
    /// Peers without typed topology fall back to the `"default"` region and
    /// zone so that they are still queryable.
    pub fn from_peers(peers: &[PeerRecord]) -> Self {
        let mut by_region: HashMap<Region, Vec<PeerRecord>> = HashMap::new();
        let mut by_zone: HashMap<Zone, Vec<PeerRecord>> = HashMap::new();
        let mut zone_to_region: HashMap<Zone, Region> = HashMap::new();

        for peer in peers {
            let (region, zone) = resolve_topology(peer);

            by_region
                .entry(region.clone())
                .or_default()
                .push(peer.clone());
            zone_to_region
                .entry(zone.clone())
                .or_insert_with(|| region.clone());
            by_zone.entry(zone).or_default().push(peer.clone());
        }

        Self {
            by_region,
            by_zone,
            zone_to_region,
        }
    }

    /// All distinct regions present in the view.
    pub fn regions(&self) -> Vec<&Region> {
        self.by_region.keys().collect()
    }

    /// Zones that belong to the given region.
    pub fn zones_in_region(&self, region: &Region) -> Vec<&Zone> {
        self.zone_to_region
            .iter()
            .filter_map(|(z, r)| if r == region { Some(z) } else { None })
            .collect()
    }

    /// All peers located in the given region (any status).
    pub fn peers_in_region(&self, region: &Region) -> &[PeerRecord] {
        self.by_region.get(region).map(Vec::as_slice).unwrap_or(&[])
    }

    /// All peers located in the given zone (any status).
    pub fn peers_in_zone(&self, zone: &Zone) -> &[PeerRecord] {
        self.by_zone.get(zone).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Number of **active** peers in the given region.
    pub fn active_count_in_region(&self, region: &Region) -> usize {
        self.peers_in_region(region)
            .iter()
            .filter(|p| p.status == PeerStatus::Active)
            .count()
    }

    /// Number of **active** peers in the given zone.
    pub fn active_count_in_zone(&self, zone: &Zone) -> usize {
        self.peers_in_zone(zone)
            .iter()
            .filter(|p| p.status == PeerStatus::Active)
            .count()
    }
}

/// Resolve the effective (region, zone) for a peer.
///
/// Prefers the typed `topology` field; falls back to the legacy string fields;
/// finally falls back to `("default", "default")`.
fn resolve_topology(peer: &PeerRecord) -> (Region, Zone) {
    if let Some(ref topo) = peer.topology {
        return (topo.region.clone(), topo.zone.clone());
    }

    let region = peer
        .region
        .as_deref()
        .and_then(Region::new)
        .unwrap_or_else(|| Region::new(DEFAULT_REGION).expect("default region is valid"));

    let zone = peer
        .zone
        .as_deref()
        .and_then(Zone::new)
        .unwrap_or_else(|| Zone::new(DEFAULT_ZONE).expect("default zone is valid"));

    (region, zone)
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv6Addr, SocketAddr};

    use syfrah_core::mesh::{PeerStatus, Topology};

    use super::*;

    fn make_peer(name: &str, region: &str, zone: &str, status: PeerStatus) -> PeerRecord {
        PeerRecord {
            name: name.to_owned(),
            wg_public_key: format!("key-{name}"),
            endpoint: SocketAddr::new(std::net::IpAddr::V6(Ipv6Addr::LOCALHOST), 51820),
            mesh_ipv6: Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1),
            last_seen: 0,
            status,
            region: Some(region.to_owned()),
            zone: Some(zone.to_owned()),
            topology: Some(Topology {
                region: Region::new(region).unwrap(),
                zone: Zone::new(zone).unwrap(),
            }),
        }
    }

    fn sample_peers() -> Vec<PeerRecord> {
        vec![
            make_peer("node-1", "eu-west", "eu-west-1a", PeerStatus::Active),
            make_peer("node-2", "eu-west", "eu-west-1a", PeerStatus::Active),
            make_peer("node-3", "eu-west", "eu-west-1b", PeerStatus::Unreachable),
            make_peer("node-4", "us-east", "us-east-1a", PeerStatus::Active),
            make_peer("node-5", "us-east", "us-east-1a", PeerStatus::Removed),
        ]
    }

    #[test]
    fn from_peers_groups_correctly() {
        let view = TopologyView::from_peers(&sample_peers());

        let mut regions: Vec<String> = view
            .regions()
            .iter()
            .map(|r| r.as_str().to_owned())
            .collect();
        regions.sort();
        assert_eq!(regions, vec!["eu-west", "us-east"]);
    }

    #[test]
    fn peers_in_region_returns_correct_slice() {
        let view = TopologyView::from_peers(&sample_peers());

        let eu = Region::new("eu-west").unwrap();
        assert_eq!(view.peers_in_region(&eu).len(), 3);

        let us = Region::new("us-east").unwrap();
        assert_eq!(view.peers_in_region(&us).len(), 2);

        let unknown = Region::new("ap-south").unwrap();
        assert_eq!(view.peers_in_region(&unknown).len(), 0);
    }

    #[test]
    fn peers_in_zone_returns_correct_slice() {
        let view = TopologyView::from_peers(&sample_peers());

        let z = Zone::new("eu-west-1a").unwrap();
        assert_eq!(view.peers_in_zone(&z).len(), 2);

        let z2 = Zone::new("eu-west-1b").unwrap();
        assert_eq!(view.peers_in_zone(&z2).len(), 1);
    }

    #[test]
    fn zones_in_region() {
        let view = TopologyView::from_peers(&sample_peers());

        let eu = Region::new("eu-west").unwrap();
        let mut zones: Vec<String> = view
            .zones_in_region(&eu)
            .iter()
            .map(|z| z.as_str().to_owned())
            .collect();
        zones.sort();
        assert_eq!(zones, vec!["eu-west-1a", "eu-west-1b"]);

        let us = Region::new("us-east").unwrap();
        let zones_us: Vec<String> = view
            .zones_in_region(&us)
            .iter()
            .map(|z| z.as_str().to_owned())
            .collect();
        assert_eq!(zones_us, vec!["us-east-1a"]);
    }

    #[test]
    fn active_counts() {
        let view = TopologyView::from_peers(&sample_peers());

        let eu = Region::new("eu-west").unwrap();
        assert_eq!(view.active_count_in_region(&eu), 2);

        let us = Region::new("us-east").unwrap();
        assert_eq!(view.active_count_in_region(&us), 1);

        let z = Zone::new("us-east-1a").unwrap();
        assert_eq!(view.active_count_in_zone(&z), 1);
    }

    #[test]
    fn empty_peers() {
        let view = TopologyView::from_peers(&[]);
        assert!(view.regions().is_empty());

        let r = Region::new("any").unwrap();
        assert_eq!(view.peers_in_region(&r).len(), 0);
        assert_eq!(view.active_count_in_region(&r), 0);

        let z = Zone::new("any").unwrap();
        assert_eq!(view.peers_in_zone(&z).len(), 0);
        assert_eq!(view.active_count_in_zone(&z), 0);
    }

    #[test]
    fn peers_without_topology_use_defaults() {
        let peer = PeerRecord {
            name: "bare".to_owned(),
            wg_public_key: "key-bare".to_owned(),
            endpoint: SocketAddr::new(std::net::IpAddr::V6(Ipv6Addr::LOCALHOST), 51820),
            mesh_ipv6: Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 1),
            last_seen: 0,
            status: PeerStatus::Active,
            region: None,
            zone: None,
            topology: None,
        };

        let view = TopologyView::from_peers(&[peer]);

        let default_region = Region::new("default").unwrap();
        assert_eq!(view.peers_in_region(&default_region).len(), 1);
        assert_eq!(view.active_count_in_region(&default_region), 1);

        let default_zone = Zone::new("default").unwrap();
        assert_eq!(view.peers_in_zone(&default_zone).len(), 1);
    }
}
