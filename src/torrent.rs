use hashes::Hashes;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
// A torrent file (also known as a metainfo file) contains a bencoded dictionary.
pub struct Torrent {
    // The URL of the tracker.
    pub announce: String,
    // This maps to a dictionary, with keys described in Info.
    pub info: Info,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Info {
    // name: a UTF-8 encoded string which is the suggested name to save the file (or directory) as
    name: String,
    // piece length: number of bytes in each piece maps to the number of bytes in each piece the file is split into.
    //
    // For the purposes of transfer, files are split into fixed-size pieces which are all the same length,
    // except for possibly the last one which may be truncated.
    #[serde(rename = "piece length")]
    plength: usize,
    // pieces: concatenated SHA-1 hashes of each piece, maps to a string whose length is a multiple of 20.
    // pieces: Vec<[u8; 20]>,
    pieces: Hashes,
    //length: size of the file in bytes, for single-file torrents.
    // If length is present then the download represents a single file,
    // otherwise it represents a set of files which go in a directory structure
    #[serde(flatten)]
    pub keys: Keys,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Keys {
    // If length is present then the download represents a single file.
    // In the single file case, length maps to the length of the file in bytes.
    SingleFile { length: usize },
    // Otherwise it represents a set of files which go in a directory structure.
    // the multi-file case is treated as only having a single file by concatenating the files in the order they appear in the files list.
    // The files list is the value files maps to, and is a list of dictionaries containing the following keys in struct File.
    MultiFile { files: Vec<File> },
}

#[derive(Debug, Clone, Deserialize)]
pub struct File {
    // length - The length of the file, in bytes.
    length: usize,
    // path - A list of UTF-8 encoded strings corresponding to subdirectory names,
    // the last of which is the actual file name (a zero length list is an error case).
    path: Vec<String>,
}

mod hashes {
    use serde::de::{self, Deserialize, Deserializer, Visitor};
    use std::fmt;

    #[derive(Debug, Clone)]
    pub struct Hashes(pub Vec<[u8; 20]>);
    struct HashesVisitor;

    // Implement Visitor pattern to deserialize SHA-1 hashes (byte array) into exact Vec<[u8, 20]>.
    impl<'de> Visitor<'de> for HashesVisitor {
        type Value = Hashes;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a byte string whose length is a multiple of 20")
        }

        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() % 20 != 0 {
                return Err(E::custom(format!("length is {}", v.len())));
            }
            // TODO: use array_chunks when stable
            Ok(Hashes(
                v.chunks_exact(20)
                    .map(|slice_20| slice_20.try_into().expect("guaranteed to be length 20"))
                    .collect(),
            ))
        }
    }

    impl<'de> Deserialize<'de> for Hashes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(HashesVisitor)
        }
    }
}
