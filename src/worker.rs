use std::ops::Range;
use std::sync::Arc;
use std::{collections::vec_deque::VecDeque, sync::Mutex};

use crate::handshake;
use crate::peer;
use crate::torrent::Torrent;

use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use sha1::{Digest, Sha1};
use tokio::{net::TcpStream, sync::mpsc::Sender};
use tokio_util::codec::Framed;

use handshake::Handshake;
use peer::{Message, MessageFrame, MessageType, Piece, Request};

pub struct Worker {
    torrent: Arc<Torrent>,
    peer: String,
}

impl Worker {
    const PEER_ID_BYTES: [u8; 20] = *b"00112233445566778899";

    // Each block max size is 16 kiB (16 * 1024 bytes)
    const BLOCK_SIZE: usize = 1 << 14;

    pub fn new(torrent: Arc<Torrent>, peer: String) -> Self {
        Self { torrent, peer }
    }

    pub async fn connect(&self) -> anyhow::Result<TcpStream> {
        let info_hash = self.torrent.info_hash()?;

        let mut handshake = Handshake::new(info_hash, Self::PEER_ID_BYTES);
        let stream = handshake.send(&self.peer).await?;

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

    pub async fn download_queue(
        &self,
        queue: PiecesQueue,
        result: Sender<(usize, Vec<u8>)>,
    ) -> anyhow::Result<()> {
        // first connect to a node
        let stream = self.connect().await?;
        let mut frame = self.init_frame(stream).await?;

        let file_len = self
            .torrent
            .info
            .file_length()
            .ok_or(anyhow::anyhow!("Multifile not implemented yet"))?;

        let num_pieces = self.torrent.info.pieces.num_pieces();

        'start: loop {
            // get piece
            let Some(piece_i) = queue.take_piece() else {
                println!("no more pieces, exiting");
                // we are done no more pieces at this time
                break;
            };

            println!("Downloading piece: {} ", piece_i);

            let piece_hash = self.torrent.info.pieces[piece_i];

            let piece_size = if piece_i == num_pieces - 1 {
                // here we need to check if piece length is equal to max piece value, or not.
                let base_len = file_len % self.torrent.info.plength;
                if base_len == 0 {
                    self.torrent.info.plength
                } else {
                    base_len
                }
            } else {
                self.torrent.info.plength
            };

            // we need to figure out what block it is, for that we have:
            // piece_i
            // num_pieces
            // piece_size
            // blocks start at 0, that it is why the -1,
            // if piece_i = 0, them we will have 2B/1B -> 1 - 1 = 0;
            let n_blocks = (piece_size + (Self::BLOCK_SIZE - 1)) / Self::BLOCK_SIZE;
            let mut piece_data = Vec::with_capacity(piece_size);

            // now need to set index, begin, length.
            // index = piece_i
            // begin would depend of n_block
            for block in 0..n_blocks {
                let block_size = if block == n_blocks - 1 {
                    // it turns out this piece is equal to max_block_len?
                    let base_len = piece_size % Self::BLOCK_SIZE;
                    if base_len == 0 {
                        Self::BLOCK_SIZE
                    } else {
                        base_len
                    }
                } else {
                    Self::BLOCK_SIZE
                };

                let request = Request {
                    index: piece_i as u32,
                    begin: (block * Self::BLOCK_SIZE) as u32,
                    length: block_size as u32,
                };

                if frame
                    .send(Message {
                        id: MessageType::Request,
                        payload: request.as_bytes().to_vec(),
                    })
                    .await
                    .is_err()
                {
                    queue.push_piece(piece_i);
                    break 'start;
                }

                // now read response
                let Some(Ok(piece)) = frame.next().await else {
                    queue.push_piece(piece_i);
                    break;
                };

                if piece.payload.is_empty() {
                    queue.push_piece(piece_i);
                    break 'start;
                }
                // assert_eq!(piece.tag, Tag::Piece);

                let Some(piece) = Piece::load_from_payload(&piece.payload) else {
                    queue.push_piece(piece_i);
                    break 'start;
                };

                if piece.index as usize != piece_i
                    || piece.begin as usize != (block * Self::BLOCK_SIZE)
                    || piece.piece.len() != block_size
                {
                    // we downloaded an invalid piece
                    queue.push_piece(piece_i);
                    break 'start;
                }

                // now extend our data
                piece_data.extend_from_slice(piece.piece);
            }

            if piece_data.len() != piece_size {
                queue.push_piece(piece_i);
                break 'start;
            }

            let mut hasher = Sha1::new();
            hasher.update(&piece_data);

            let hash: Result<[u8; 20], _> = hasher.finalize().try_into();
            let Ok(hash) = hash else {
                queue.push_piece(piece_i);
                break 'start;
            };

            if hash != piece_hash {
                queue.push_piece(piece_i);
                break 'start;
            }

            // This will errors only if receiver was closed before.
            // so no need to push unsuccesful piece id
            result.send((piece_i, piece_data)).await?;
        }

        Ok(())
    }
}

fn get_residual_size(index: usize, count: usize, length: usize, max_length: usize) -> usize {
    if index == count - 1 && (length % max_length) != 0 {
        length % max_length
    } else {
        max_length
    }
}

#[derive(Clone, Debug)]
pub struct PiecesQueue(Arc<Mutex<VecDeque<usize>>>);

impl PiecesQueue {
    pub fn new(pieces: Range<usize>) -> Self {
        let queue = pieces.collect::<VecDeque<usize>>();
        let pieces = Arc::new(Mutex::new(queue));
        Self(pieces)
    }

    pub fn take_piece(&self) -> Option<usize> {
        self.0.lock().expect("PiecesQueue take piece").pop_front()
    }

    pub fn push_piece(&self, piece: usize) {
        self.0
            .lock()
            .expect("PiecesQueue push piece")
            .push_back(piece)
    }
}
