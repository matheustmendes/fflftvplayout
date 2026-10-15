//! What the dashboard needs to know to say that something is wrong before the
//! audience does: whether the filler exists and which destinations are
//! connected to the relay.

use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{
    db::models::{MonitorDestination, RelayConfig, RelayProtocol},
    utils::config::PlayoutConfig,
};

const RELAY_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
pub struct FillerStatus {
    pub path: String,
    /// There is a file (or a folder with files) to play when nothing else can.
    pub ok: bool,
}

/// The filler plays when a clip fails or a day has no playlist; without it the
/// picture goes black.
pub fn filler_status(config: &PlayoutConfig) -> FillerStatus {
    let path = &config.storage.filler_path;
    let ok = if path.is_dir() {
        std::fs::read_dir(path).is_ok_and(|mut entries| entries.any(|entry| entry.is_ok()))
    } else {
        path.is_file()
    };

    FillerStatus {
        path: config.storage.filler.clone(),
        ok,
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connected,
    Disconnected,
    /// The relay could not be asked.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct DestinationStatus {
    pub id: i32,
    pub name: String,
    pub protocol: RelayProtocol,
    pub state: ConnectionState,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelayStatus {
    /// An address of the relay API was given.
    pub configured: bool,
    pub reachable: bool,
    pub destinations: Vec<DestinationStatus>,
    /// Readers of the channel that no destination of the list explains.
    pub others: usize,
}

/// One connection as the relay API lists it. Only what is needed is read: the
/// query also carries passwords, so it never leaves this module.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RelayConnection {
    pub remote_addr: String,
    pub state: String,
    pub path: String,
    pub query: String,
}

#[derive(Debug, Deserialize)]
struct ConnectionList {
    #[serde(default)]
    items: Vec<RelayConnection>,
}

impl RelayConnection {
    fn user(&self) -> Option<&str> {
        self.query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find_map(|(key, value)| (key == "user").then_some(value))
    }

    fn host(&self) -> &str {
        let address = self.remote_addr.as_str();

        // `[::1]:5000` and `1.2.3.4:5000`
        if let Some(rest) = address.strip_prefix('[') {
            return rest.split(']').next().unwrap_or_default();
        }

        address.rsplit_once(':').map_or(address, |(host, _)| host)
    }

    fn is_reading(&self, path: &str) -> bool {
        self.state == "read" && self.path.trim_matches('/') == path.trim_matches('/')
    }

    fn is_destination(&self, destination: &MonitorDestination) -> bool {
        let user = destination.match_user.trim();
        let address = destination.match_address.trim();

        (user.is_empty() || self.user() == Some(user))
            && (address.is_empty() || self.host() == address)
    }
}

/// Decide the state of every destination from the connections the relay lists.
pub fn relay_status(
    config: &RelayConfig,
    rtmp: &[RelayConnection],
    srt: &[RelayConnection],
) -> RelayStatus {
    let readers = |connections: &[RelayConnection]| -> Vec<RelayConnection> {
        connections
            .iter()
            .filter(|connection| connection.is_reading(&config.path))
            .cloned()
            .collect()
    };
    let rtmp = readers(rtmp);
    let srt = readers(srt);
    let destinations: Vec<DestinationStatus> = config
        .destinations
        .iter()
        .filter(|destination| destination.enabled)
        .map(|destination| {
            let connections = match destination.protocol {
                RelayProtocol::Rtmp => &rtmp,
                RelayProtocol::Srt => &srt,
            };
            let connected = connections
                .iter()
                .any(|connection| connection.is_destination(destination));

            DestinationStatus {
                id: destination.id,
                name: destination.name.clone(),
                protocol: destination.protocol,
                state: if connected {
                    ConnectionState::Connected
                } else {
                    ConnectionState::Disconnected
                },
            }
        })
        .collect();
    let others = rtmp
        .iter()
        .map(|connection| (RelayProtocol::Rtmp, connection))
        .chain(
            srt.iter()
                .map(|connection| (RelayProtocol::Srt, connection)),
        )
        .filter(|(protocol, connection)| {
            !config.destinations.iter().any(|destination| {
                destination.enabled
                    && destination.protocol == *protocol
                    && connection.is_destination(destination)
            })
        })
        .count();

    RelayStatus {
        configured: true,
        reachable: true,
        destinations,
        others,
    }
}

fn unreachable_status(config: &RelayConfig) -> RelayStatus {
    RelayStatus {
        configured: true,
        reachable: false,
        destinations: config
            .destinations
            .iter()
            .filter(|destination| destination.enabled)
            .map(|destination| DestinationStatus {
                id: destination.id,
                name: destination.name.clone(),
                protocol: destination.protocol,
                state: ConnectionState::Unknown,
            })
            .collect(),
        others: 0,
    }
}

async fn fetch_connections(
    client: &Client,
    base: &str,
    kind: &str,
) -> Result<Vec<RelayConnection>, reqwest::Error> {
    let list: ConnectionList = client
        .get(format!("{base}/v3/{kind}/list?itemsPerPage=1000"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(list.items)
}

/// Ask the relay who is connected. A relay that does not answer is reported as
/// unreachable instead of as a failure: that is something the screen shows.
pub async fn check_relay(config: &RelayConfig) -> RelayStatus {
    let base = config.api_url.trim().trim_end_matches('/');

    if base.is_empty() {
        return RelayStatus {
            configured: false,
            reachable: false,
            destinations: Vec::new(),
            others: 0,
        };
    }

    // The client needs a TLS provider even for a plain http address; installing
    // it twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let Ok(client) = Client::builder().timeout(RELAY_TIMEOUT).build() else {
        return unreachable_status(config);
    };
    let (rtmp, srt) = tokio::join!(
        fetch_connections(&client, base, "rtmpconns"),
        fetch_connections(&client, base, "srtconns")
    );

    match (rtmp, srt) {
        (Ok(rtmp), Ok(srt)) => relay_status(config, &rtmp, &srt),
        _ => unreachable_status(config),
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, routing::get};
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;

    fn connection(addr: &str, state: &str, path: &str, query: &str) -> RelayConnection {
        RelayConnection {
            remote_addr: addr.to_string(),
            state: state.to_string(),
            path: path.to_string(),
            query: query.to_string(),
        }
    }

    fn destination(
        id: i32,
        protocol: RelayProtocol,
        user: &str,
        address: &str,
    ) -> MonitorDestination {
        MonitorDestination {
            id,
            channel_id: 1,
            name: format!("destino {id}"),
            protocol,
            match_user: user.to_string(),
            match_address: address.to_string(),
            enabled: true,
        }
    }

    fn config(destinations: Vec<MonitorDestination>) -> RelayConfig {
        RelayConfig {
            api_url: "http://relay".to_string(),
            path: "canal1".to_string(),
            destinations,
        }
    }

    #[test]
    fn a_destination_is_found_by_its_user_or_its_address() {
        let config = config(vec![
            destination(1, RelayProtocol::Rtmp, "praia", ""),
            destination(2, RelayProtocol::Srt, "", "200.1.1.9"),
            destination(3, RelayProtocol::Rtmp, "serra", ""),
        ]);
        let rtmp = [
            connection("10.0.0.5:5000", "read", "canal1", "user=praia&pass=segredo"),
            connection("10.0.0.6:5000", "read", "outro", "user=serra"),
        ];
        let srt = [connection("200.1.1.9:7000", "read", "canal1", "")];
        let status = relay_status(&config, &rtmp, &srt);
        let states: Vec<_> = status.destinations.iter().map(|d| d.state).collect();

        assert_eq!(
            states,
            [
                ConnectionState::Connected,
                ConnectionState::Connected,
                ConnectionState::Disconnected,
            ]
        );
        assert_eq!(status.others, 0);
    }

    #[test]
    fn publishers_and_unknown_readers_are_not_destinations() {
        let config = config(vec![destination(1, RelayProtocol::Rtmp, "praia", "")]);
        let rtmp = [
            connection("127.0.0.1:1", "publish", "canal1", ""),
            connection("10.0.0.5:5000", "read", "canal1", "user=intruso"),
        ];
        let status = relay_status(&config, &rtmp, &[]);

        assert_eq!(status.destinations[0].state, ConnectionState::Disconnected);
        assert_eq!(status.others, 1, "only the unexplained reader counts");
    }

    #[test]
    fn ipv6_hosts_and_disabled_destinations_are_handled() {
        let mut off = destination(2, RelayProtocol::Srt, "", "10.9.9.9");

        off.enabled = false;

        let config = config(vec![
            destination(1, RelayProtocol::Srt, "", "2001:db8::1"),
            off,
        ]);
        let srt = [connection("[2001:db8::1]:9000", "read", "canal1", "")];
        let status = relay_status(&config, &[], &srt);

        assert_eq!(status.destinations.len(), 1);
        assert_eq!(status.destinations[0].state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn the_relay_api_is_read_and_a_dead_relay_is_unknown() {
        let app = Router::new()
            .route(
                "/v3/rtmpconns/list",
                get(|| async {
                    axum::Json(json!({ "items": [
                        { "remoteAddr": "10.0.0.5:1", "state": "read", "path": "canal1",
                          "query": "user=praia", "id": "x", "created": "now" }
                    ]}))
                }),
            )
            .route(
                "/v3/srtconns/list",
                get(|| async { axum::Json(json!({ "items": [] })) }),
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut live = config(vec![destination(1, RelayProtocol::Rtmp, "praia", "")]);

        live.api_url = format!("http://{address}/");

        let status = check_relay(&live).await;

        assert!(status.reachable);
        assert_eq!(status.destinations[0].state, ConnectionState::Connected);

        // Nobody listens on port 1.
        live.api_url = "http://127.0.0.1:1".to_string();

        let status = check_relay(&live).await;

        assert!(status.configured && !status.reachable);
        assert_eq!(status.destinations[0].state, ConnectionState::Unknown);

        live.api_url = String::new();
        assert!(!check_relay(&live).await.configured);
    }

    #[tokio::test]
    async fn the_filler_is_missing_until_a_file_exists() {
        let (manager, _) = crate::test_support::channel_manager().await;
        let mut config = manager.config.read().await.clone();
        let folder =
            std::env::temp_dir().join(format!("ffplayout-filler-{}", uuid::Uuid::new_v4()));

        std::fs::create_dir_all(&folder).unwrap();
        config.storage.filler_path = folder.join("filler.mp4");
        assert!(!filler_status(&config).ok);

        std::fs::write(&config.storage.filler_path, b"x").unwrap();
        assert!(filler_status(&config).ok);

        // A folder counts when it holds something.
        config.storage.filler_path = folder.clone();
        assert!(filler_status(&config).ok);

        let empty = folder.join("empty");

        std::fs::create_dir_all(&empty).unwrap();
        config.storage.filler_path = empty;
        assert!(!filler_status(&config).ok);

        std::fs::remove_dir_all(&folder).unwrap();
    }
}
