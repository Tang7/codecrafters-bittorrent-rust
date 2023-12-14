// The handshake is a message consisting of the following parts as described in the peer protocol:

use std::net::SocketAddrV4;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

// length of the protocol string (BitTorrent protocol) which is 19 (1 byte)
// the string BitTorrent protocol (19 bytes)
// eight reserved bytes, which are all set to zero (8 bytes)
// sha1 infohash (20 bytes) (NOT the hexadecimal representation, which is 40 bytes long)
// peer id (20 bytes) (you can use 00112233445566778899 for this challenge)
pub struct Handshake {
    length: u8,
    protocol: [u8; 19],
    reserved: [u8; 8],
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

impl Handshake {
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            length: 19,
            protocol: *b"BitTorrent protocol",
            reserved: [0; 8],
            info_hash,
            peer_id,
        }
    }

    pub fn as_bytes(&self) -> [u8; 68] {
        let mut bytes = [0u8; 68];
        bytes[0] = self.length;
        bytes[1..20].copy_from_slice(&self.protocol);
        bytes[20..28].copy_from_slice(&self.reserved);
        bytes[28..48].copy_from_slice(&self.info_hash);
        bytes[48..68].copy_from_slice(&self.peer_id);
        bytes
    }

    pub async fn send(&mut self, peer: &str) -> anyhow::Result<TcpStream> {
        let peer = peer.parse::<SocketAddrV4>()?;
        let mut stream = tokio::net::TcpStream::connect(peer).await?;
        // TODO: how to change handshake inplace to avoid copy.
        let mut handshake_bytes = self.as_bytes();
        stream.write_all(&handshake_bytes).await?;
        stream.read_exact(&mut handshake_bytes).await?;

        if handshake_bytes[28..48] != self.info_hash {
            return Err(anyhow::anyhow!("Mismatched info hash from handshake"));
        }

        self.peer_id = handshake_bytes[48..68].try_into().unwrap();

        Ok(stream)
    }
}
