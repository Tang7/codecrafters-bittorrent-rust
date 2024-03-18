mod handshake;
mod peer;
mod torrent;
mod tracker;

use anyhow::Context;
use handshake::Handshake;
use peer::{Message, MessageFrame, MessageType, Piece, Request};
use sha1::{Digest, Sha1};
use torrent::read_torrent_file;
use tracker::TrackerRequest;

use bittorrent_starter_rust::bencode;
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;

const PEER_ID: &str = "00112233445566778899";
const PEER_ID_BYTES: [u8; 20] = *b"00112233445566778899";

// Each block max size is 16 kiB (16 * 1024 bytes)
const BLOCK_SIZE: usize = 1 << 14;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
#[clap(rename_all = "snake_case")]
enum Command {
    Decode {
        value: String,
    },
    Info {
        torrent: PathBuf,
    },
    Peers {
        torrent: PathBuf,
    },
    Handshake {
        torrent: PathBuf,
        peer: String,
    },
    DownloadPiece {
        #[arg(short)]
        output: String,
        torrent: PathBuf,
        piece: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Decode { value } => {
            let decoded_value = bencode::decode_bencoded_value(&value).0;
            println!("{decoded_value}");
        }
        Command::Info { torrent } => {
            let torrent_file = read_torrent_file(torrent)?;

            println!("Tracker URL: {}", torrent_file.announce);
            if let torrent::Keys::SingleFile { length } = torrent_file.info.keys {
                println!("Length: {length}");
            } else {
                todo!();
            }

            let info_hash = torrent_file.info_hash()?;
            println!("Info Hash: {}", hex::encode(&info_hash));
            println!("Piece Length: {}", torrent_file.info.plength);
            println!("Piece Hashes:");
            for hash in torrent_file.info.pieces.0 {
                println!("{}", hex::encode(&hash));
            }
        }
        Command::Peers { torrent } => {
            let torrent_file = read_torrent_file(torrent)?;

            let length = if let torrent::Keys::SingleFile { length } = torrent_file.info.keys {
                length
            } else {
                todo!();
            };

            let info_hash = torrent_file.info_hash()?;

            let req = TrackerRequest::new(PEER_ID, length);
            let resp = req.send(&torrent_file.announce, info_hash).await?;
            for peer in resp.peers.0 {
                println!("{}:{}", peer.ip(), peer.port());
            }
        }
        Command::Handshake { torrent, peer } => {
            let torrent_file = read_torrent_file(torrent)?;

            let info_hash = torrent_file.info_hash()?;

            let mut handshake = Handshake::new(info_hash, PEER_ID_BYTES);
            handshake.send(&peer).await?;

            println!("Peer ID: {}", hex::encode(handshake.peer_id));
        }
        Command::DownloadPiece {
            output: out_path,
            torrent,
            piece: piece_id,
        } => {
            let torrent_file = read_torrent_file(torrent)?;
            let info_hash = torrent_file.info_hash()?;

            let length = if let torrent::Keys::SingleFile { length } = torrent_file.info.keys {
                length
            } else {
                todo!();
            };

            let req = TrackerRequest::new(PEER_ID, length);
            let resp = req.send(&torrent_file.announce, info_hash).await?;

            let peer_addr = format!("{}", resp.peers.0[0]);

            let mut handshake = Handshake::new(info_hash, PEER_ID_BYTES);
            let handshake_stream = handshake.send(&peer_addr).await?;

            let mut peer = tokio_util::codec::Framed::new(handshake_stream, MessageFrame);
            let bitfield_msg = peer
                .next()
                .await
                .expect("wait bitfield message")
                .context("invalid message")?;

            assert_eq!(MessageType::Bitfield, bitfield_msg.id);

            peer.send(Message {
                id: MessageType::Interested,
                payload: Vec::new(),
            })
            .await
            .context("send interested message")?;

            let unchoke = peer
                .next()
                .await
                .expect("wait unchoke message")
                .context("invalid message while waiting unchoke")?;

            assert_eq!(MessageType::Unchoke, unchoke.id);
            assert!(unchoke.payload.is_empty());

            // Start download piece speficied by piece id.
            let num_pieces = torrent_file.info.pieces.0.len();
            assert!(piece_id < num_pieces);
            let piece_hash = torrent_file.info.pieces.0[piece_id];
            // Last piece may not equal to defined plength.
            let piece_size =
                if piece_id == num_pieces - 1 && (length % torrent_file.info.plength) != 0 {
                    length % torrent_file.info.plength
                } else {
                    torrent_file.info.plength
                };

            // Break the piece into blocks of 16 kiB (16 * 1024 bytes) and send a request message for each block
            let num_blocks = (piece_size + (BLOCK_SIZE - 1)) / BLOCK_SIZE;
            let mut block_data = Vec::with_capacity(piece_size);
            for block in 0..num_blocks {
                // The last block will contain 2^14 bytes or less, need to calculate this value using the piece length.
                let block_size = if block == num_blocks - 1 && (piece_size % BLOCK_SIZE) != 0 {
                    piece_size % BLOCK_SIZE
                } else {
                    BLOCK_SIZE
                };

                let request = Request {
                    index: piece_id as u32,
                    begin: (block * BLOCK_SIZE) as u32,
                    length: block_size as u32,
                };

                peer.send(Message {
                    id: MessageType::Request,
                    payload: request.as_bytes().to_vec(),
                })
                .await
                .context("send request message")?;

                let piece_msg = peer
                    .next()
                    .await
                    .expect("wait request response")
                    .context("invalid request response")?;

                assert!(!piece_msg.payload.is_empty());

                let piece_data = Piece::load_from_payload(&piece_msg.payload)
                    .ok_or(anyhow::anyhow!("Invalid piece from peer"))?;
                assert_eq!(piece_data.index as usize, piece_id);
                assert_eq!(piece_data.begin as usize, block * BLOCK_SIZE);
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
            assert_eq!(hash, piece_hash);

            tokio::fs::write(&out_path, block_data).await?;
            println!("Piece {} downloaded to {}.", piece_id, out_path);
        }
    }

    Ok(())
}
