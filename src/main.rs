use anyhow::Context;
use clap::{Parser, Subcommand};
use serde_bencode;
use std::path::PathBuf;

mod bencode;
mod torrent;

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
        }
    }

    Ok(())
}
