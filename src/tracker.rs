use serde::{Deserialize, Serialize};

pub use peers::Peers;

use crate::torrent::Torrent;

#[derive(Debug, Clone, Serialize)]
pub struct TrackerRequest {
    // peer_id: a unique identifier for your client
    //
    // A string of length 20 that you get to pick. You can use something like 00112233445566778899.
    pub peer_id: String,

    // port: the port your client is listening on
    //
    // You can set this to 6881, you will not have to support this functionality during this challenge.
    pub port: u16,

    // uploaded: the total amount uploaded so far
    //
    // Since your client hasn't uploaded anything yet, you can setPeersVisitor.
    pub uploaded: usize,

    // downloaded: the total amount downloaded so far
    //
    // Since your client hasn't downloaded anything yet, you can set this to 0.
    pub downloaded: usize,

    // left: the number of bytes left to download
    //
    // Since you client hasn't downloaded anything yet, this'll be the total length of the file (you've extracted this value from the torrent file in previous stages)
    pub left: usize,

    // compact: whether the peer list should use the compact representation
    //
    // For the purposes of this challenge, set this to 1.
    // The compact representation is more commonly used in the wild, the non-compact representation is mostly supported for backward-compatibility.
    pub compact: u8,
}

impl TrackerRequest {
    pub const TRACKER_PORT: u16 = 6881;

    pub fn new(peer_id: &str, left: usize) -> Self {
        Self {
            peer_id: peer_id.to_owned(),
            port: TrackerRequest::TRACKER_PORT,
            uploaded: 0,
            downloaded: 0,
            left: left,
            compact: 1,
        }
    }

    pub async fn send(
        &self,
        url: &str,
        info_hash: [u8; Torrent::HASH_SIZE],
    ) -> anyhow::Result<TrackerResponse> {
        let request_params = serde_urlencoded::to_string(&self)?;

        let tracker_url = format!(
            "{}?{}&info_hash={}",
            url,
            request_params,
            &urlencode(&info_hash)
        );

        let response = reqwest::get(tracker_url).await?;
        let response = response.bytes().await?;

        serde_bencode::from_bytes(&response).map_err(|e| anyhow::anyhow!(e))
    }
}

// Let's say the hexadecimal representation of our info hash is d69f91e6b2ae4c542468d1073a71d4ea13879a7f
// This 40 character long string was representing 20 bytes, so each character pair corresponds to a byte
// We can just put a % before each byte so the URL-encoded representation would be:%d6%9f%91%e6%b2%ae%4c%54%24%68%d1%07%3a%71%d4%ea%13%87%9a%7f
// The result is 60 characters long.
fn urlencode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}

// The tracker's response will be a bencoded dictionary.
#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    // interval:
    // An integer, indicating how often your client should make a request to the tracker.
    // You can ignore this value for the purposes of this challenge.
    pub interval: usize,

    // peers.
    // A string, which contains list of peers that your client can connect to.
    // Each peer is represented using 6 bytes. The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number.
    pub peers: Peers,
}

mod peers {
    use serde::de::{self, Deserialize, Deserializer, Visitor};
    use std::fmt;
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::vec::IntoIter;

    #[derive(Debug, Clone)]
    pub struct Peers(pub Vec<SocketAddrV4>);

    struct PeersVisitor;

    impl<'de> Visitor<'de> for PeersVisitor {
        type Value = Peers;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an IPv4 socket address, first 4 bytes are peer's IP address, last 2 bytes are the peer's port number")
        }

        fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.len() % 6 != 0 {
                return Err(E::custom(format!("invalid length: {}", value.len())));
            }

            Ok(Peers(
                value
                    .chunks_exact(6)
                    .map(|slice_6| {
                        SocketAddrV4::new(
                            Ipv4Addr::new(slice_6[0], slice_6[1], slice_6[2], slice_6[3]),
                            u16::from_be_bytes([slice_6[4], slice_6[5]]),
                        )
                    })
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Peers {
        fn deserialize<D>(deserializer: D) -> Result<Peers, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(PeersVisitor)
        }
    }

    impl Peers {
        // Method to return an iterator over the peers
        pub fn into_iter(self) -> IntoIter<SocketAddrV4> {
            self.0.into_iter()
        }
    }
}
