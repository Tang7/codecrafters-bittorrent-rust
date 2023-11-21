// The handshake is a message consisting of the following parts as described in the peer protocol:

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
    peer_id: [u8; 20],
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
}
