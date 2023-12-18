mod handshake;
mod peer;
mod torrent;
mod tracker;

use anyhow::Context;
use handshake::Handshake;
use peer::{MessageFrame, MessageType};
use torrent::read_torrent_file;
use tracker::TrackerRequest;

use bittorrent_starter_rust::bencode;
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use std::path::PathBuf;

const PEER_ID: &str = "00112233445566778899";
const PEER_ID_BYTES: [u8; 20] = *b"00112233445566778899";

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
        output: PathBuf,
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
            output,
            torrent,
            piece,
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
        }
    }

    Ok(())
}
