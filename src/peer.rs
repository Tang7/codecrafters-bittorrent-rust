use bytes::BufMut;
use bytes::{Buf, BytesMut};
use tokio_util::codec::Decoder;
use tokio_util::codec::Encoder;

// All non-keepalive messages start with a single byte which gives their type.
// The possible values are:

// 0 - choke
// 1 - unchoke
// 2 - interested
// 3 - not interested
// 4 - have
// 5 - bitfield
// 6 - request
// 7 - piece
// 8 - cancel
// 'choke', 'unchoke', 'interested', and 'not interested' have no payload.

#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    Choke = 0,
    Unchode = 1,
    Interested = 2,
    NotIntereted = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: MessageType,
    pub payload: Vec<u8>,
}

impl Message {
    // All current implementations use 2^14 (16 kiB), and close connections which request an amount greater than that.
    pub const MAX: usize = 1 << 16;
}
pub struct MessageFrame;

impl Decoder for MessageFrame {
    type Item = Message;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Messages of length zero are keepalives, and ignored.
        // Keepalives are generally sent once every two minutes, but note that timeouts can be done much more quickly when data is expected.
        if src.len() < 4 {
            return Ok(None);
        }

        // Peer messages consist of a message length prefix (4 bytes), message id (1 byte) and a payload (variable size).
        // Read length marker.
        let mut length_bytes = [0; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;

        if length == 0 {
            src.advance(4);
            return self.decode(src);
        }

        if src.len() < 5 {
            return Ok(None);
        }

        // Check that the length is not too large to avoid a denial of
        // service attack where the server runs out of memory.
        if length > Message::MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large to decode.", length),
            ));
        }

        if src.len() < 4 + length {
            // The full string has not yet arrived.
            //
            // We reserve more space in the buffer. This is not strictly
            // necessary, but is a good idea performance-wise.
            src.reserve(4 + length - src.len());

            // We inform the Framed that we need more bytes to form the next
            // frame.
            return Ok(None);
        }

        // All non-keepalive messages start with a single byte which gives their type.
        let msg_type = match src[4] {
            0 => MessageType::Choke,
            1 => MessageType::Unchode,
            2 => MessageType::Interested,
            3 => MessageType::NotIntereted,
            4 => MessageType::Have,
            5 => MessageType::Bitfield,
            6 => MessageType::Request,
            7 => MessageType::Piece,
            8 => MessageType::Cancel,
            msg_type => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Unkown message type {}.", msg_type),
                ))
            }
        };

        let data = if src.len() > 5 {
            src[5..4 + length].to_vec()
        } else {
            Vec::new()
        };

        src.advance(4 + length);

        Ok(Some(Message {
            id: msg_type,
            payload: data,
        }))
    }
}

impl Encoder<Message> for MessageFrame {
    type Error = std::io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Don't send a string if it is longer than the other end will
        // accept.
        if item.payload.len() + 1 > Message::MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", item.payload.len() + 1),
            ));
        }

        // Convert the length into a byte array.
        // The cast to u32 cannot overflow due to the length check above.
        let len_slice = u32::to_le_bytes(item.payload.len() as u32 + 1);

        // Reserve space in the buffer.
        dst.reserve(4 /* length */ + 1 /* tag */ + item.payload.len());

        // Write the length and string to the buffer.
        dst.extend_from_slice(&len_slice);
        dst.put_u8(item.id as u8);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}
