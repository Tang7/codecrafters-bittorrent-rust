mod torrent;
mod tracker;

use crate::torrent::read_torrent_file;
use crate::tracker::TrackerRequest;
use anyhow::Context;
use bittorrent_starter_rust::bencode;
use bittorrent_starter_rust::handshake::Handshake;
use clap::{Parser, Subcommand};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Decode { value: String },
    Info { torrent: PathBuf },
    Peers { torrent: PathBuf },
    Handshake { torrent: PathBuf, peer: String },
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

            let req = TrackerRequest::new("00112233445566778899", length);
            let resp = req.send(&torrent_file.announce, info_hash).await?;
            for peer in resp.peers.0 {
                println!("{}:{}", peer.ip(), peer.port());
            }
        }
        Command::Handshake { torrent, peer } => {
            let torrent_file = read_torrent_file(torrent)?;

            let info_hash = torrent_file.info_hash()?;

            let peer = peer.parse::<SocketAddrV4>().context("parse peer address")?;
            let mut conn = tokio::net::TcpStream::connect(peer)
                .await
                .context("connect to peer")?;

            let handshake = Handshake::new(info_hash, *b"00112233445566778899");
            let mut handshake_bytes = handshake.as_bytes();
            conn.write_all(&handshake_bytes)
                .await
                .context("write handshake")?;
            conn.read_exact(&mut handshake_bytes)
                .await
                .context("read handshake")?;

            if handshake_bytes[28..48] != handshake.info_hash {
                eprintln!("mismatched info hash.")
            }

            println!("Peer ID: {}", hex::encode(&handshake_bytes[48..68]));
        }
    }

    Ok(())
}
