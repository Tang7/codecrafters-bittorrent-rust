mod torrent;

use crate::torrent::read_torrent_file;
use anyhow::Context;
use bittorrent_starter_rust::bencode;
use bittorrent_starter_rust::handshake::Handshake;
use bittorrent_starter_rust::tracker::*;
use clap::{Parser, Subcommand};
use serde_bencode;
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

            let request = TrackerRequest {
                peer_id: String::from("00112233445566778899"),
                port: 6881,
                uploaded: 0,
                downloaded: 0,
                left: length,
                compact: 1,
            };

            let request_params =
                serde_urlencoded::to_string(&request).context("encoded request params")?;

            let tracker_url = format!(
                "{}?{}&info_hash={}",
                torrent_file.announce,
                request_params,
                &urlencode(&info_hash)
            );
            // println!("{tracker_url}");

            let response = reqwest::get(tracker_url)
                .await
                .context("send tracker get request")?;
            let response = response
                .bytes()
                .await
                .context("convert response into bytes")?;
            let tracker_response: TrackerResponse =
                serde_bencode::from_bytes(&response).context("parse tracker response")?;
            for peer in tracker_response.peers.0 {
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
