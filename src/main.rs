use anyhow::Context;
use bittorrent_starter_rust::bencode;
use bittorrent_starter_rust::torrent;
use clap::{Parser, Subcommand};
use serde_bencode;
use sha1::{Digest, Sha1};
use std::path::PathBuf;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Decode { value: String },
    Info { torrent: PathBuf },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Decode { value } => {
            let decoded_value = bencode::decode_bencoded_value(&value).0;
            println!("{decoded_value}");
        }
        Command::Info { torrent } => {
            let torrent_file = std::fs::read(torrent).context("read torrent file")?;
            let t: torrent::Torrent =
                serde_bencode::from_bytes(&torrent_file).context("parse torrent file")?;
            // eprintln!("{t:?}");
            println!("Tracker URL: {}", t.announce);
            if let torrent::Keys::SingleFile { length } = t.info.keys {
                println!("Length: {length}");
            } else {
                todo!();
            }

            let info_encoded = serde_bencode::to_bytes(&t.info).context("get encoded info")?;
            let mut hasher = Sha1::new();
            hasher.update(&info_encoded);
            let info_hash = hasher.finalize();
            println!("Info Hash: {}", hex::encode(&info_hash));
            println!("Piece Length: {}", t.info.plength);
            println!("Piece Hashes:");
            for hash in t.info.pieces.0 {
                println!("{}", hex::encode(&hash));
            }
        }
    }

    Ok(())
}
