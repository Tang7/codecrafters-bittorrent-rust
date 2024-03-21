use crate::handshake;
use crate::peer;
use crate::torrent::Keys;
use crate::torrent::Torrent;
use crate::tracker;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use sha1::{Digest, Sha1};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;

use handshake::Handshake;
use peer::{Message, MessageFrame, MessageType, Piece, Request};
use tracker::TrackerRequest;

pub struct Worker {
    torrent: Torrent,
}

impl Worker {
    const PEER_ID: &'static str = "00112233445566778899";
    const PEER_ID_BYTES: [u8; 20] = *b"00112233445566778899";

    // Each block max size is 16 kiB (16 * 1024 bytes)
    const BLOCK_SIZE: usize = 1 << 14;

    pub fn new(torrent: Torrent) -> Self {
        Self { torrent }
    }

    pub async fn connect(&self) -> anyhow::Result<TcpStream> {
        let info_hash = self.torrent.info_hash()?;

        let length = if let Keys::SingleFile { length } = self.torrent.info.keys {
            length
        } else {
            todo!();
        };

        let req = TrackerRequest::new(Self::PEER_ID, length);
        let resp = req.send(&self.torrent.announce, info_hash).await?;

        let peer_addr = format!("{}", resp.peers.0[0]);

        let mut handshake = Handshake::new(info_hash, Self::PEER_ID_BYTES);
        let stream = handshake.send(&peer_addr).await?;

        Ok(stream)
    }

    pub async fn init_frame(
        &self,
        stream: TcpStream,
    ) -> anyhow::Result<Framed<TcpStream, MessageFrame>> {
        let mut frame = tokio_util::codec::Framed::new(stream, MessageFrame);
        let bitfield_msg = frame
            .next()
            .await
            .expect("wait bitfield message")
            .context("invalid message")?;

        assert_eq!(MessageType::Bitfield, bitfield_msg.id);

        frame
            .send(Message {
                id: MessageType::Interested,
                payload: Vec::new(),
            })
            .await
            .context("send interested message")?;

        let unchoke = frame
            .next()
            .await
            .expect("wait unchoke message")
            .context("invalid message while waiting unchoke")?;

        assert_eq!(MessageType::Unchoke, unchoke.id);
        assert!(unchoke.payload.is_empty());

        Ok(frame)
    }

    pub async fn download_piece(&self, piece_id: usize) -> anyhow::Result<Vec<u8>> {
        let stream = self.connect().await?;
        let mut frame = self.init_frame(stream).await?;

        // Start download piece speficied by piece id.
        let num_pieces = self.torrent.info.pieces.0.len();
        assert!(piece_id < num_pieces);

        let length = self
            .torrent
            .info
            .file_length()
            .ok_or(anyhow::anyhow!("MultiFile is unsupported"))?;

        // Last piece may not equal to defined plength.
        let piece_size = get_residual_size(piece_id, num_pieces, length, self.torrent.info.plength);

        // Break the piece into blocks of 16 kiB (16 * 1024 bytes) and send a request message for each block
        let num_blocks = (piece_size + (Self::BLOCK_SIZE - 1)) / Self::BLOCK_SIZE;

        let mut block_data = Vec::with_capacity(piece_size);

        for block in 0..num_blocks {
            // The last block will contain 2^14 bytes or less, need to calculate this value using the max block size.
            let block_size = get_residual_size(block, num_blocks, piece_size, Self::BLOCK_SIZE);

            let request = Request {
                index: piece_id as u32,
                begin: (block * Self::BLOCK_SIZE) as u32,
                length: block_size as u32,
            };

            frame
                .send(Message {
                    id: MessageType::Request,
                    payload: request.as_bytes().to_vec(),
                })
                .await
                .context("send request message")?;

            let piece_msg = frame
                .next()
                .await
                .expect("wait request response")
                .context("invalid request response")?;

            assert!(!piece_msg.payload.is_empty());

            let piece_data = Piece::load_from_payload(&piece_msg.payload)
                .ok_or(anyhow::anyhow!("Invalid piece from peer"))?;
            assert_eq!(piece_data.index as usize, piece_id);
            assert_eq!(piece_data.begin as usize, block * Self::BLOCK_SIZE);
            assert_eq!(piece_data.piece.len(), block_size);

            block_data.extend_from_slice(piece_data.piece);
        }

        assert_eq!(block_data.len(), piece_size);

        // Check hash before writing data into file.
        let mut hasher = Sha1::new();
        hasher.update(&block_data);
        let hash: [u8; 20] = hasher
            .finalize()
            .try_into()
            .expect("cannot get hash from block data");
        let piece_hash = self.torrent.info.pieces.0[piece_id];
        assert_eq!(hash, piece_hash);

        Ok(block_data)
    }
}

fn get_residual_size(index: usize, count: usize, length: usize, max_length: usize) -> usize {
    if index == count - 1 && (length % max_length) != 0 {
        length % max_length
    } else {
        max_length
    }
}
