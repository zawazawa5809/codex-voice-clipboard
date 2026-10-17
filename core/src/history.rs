//! Incremental reader for `transcription-history.jsonl` (spec 1.2).

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::identity::{file_identity, FileIdentity};

const MAX_LINE: usize = 16 * 1024 * 1024;
const CHECKPOINT: u64 = 64;
const RECENT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    pub id: String,
    pub text: String,
}

impl Transcript {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { id: id.into(), text: text.into() }
    }
}

#[derive(Debug, Default)]
pub struct HistoryBatch {
    pub new_records: Vec<Transcript>,
    pub invalid_lines: usize,
}

/// Single-consumer reader. Never opens the source for writing.
///
/// The first successful read is the pre-launch baseline: it fills [`recent`](Self::recent)
/// but returns no new records. A missing file ends the baseline (anything created later is
/// new); any other read failure does not, so history that was unreadable at launch is not
/// replayed once it becomes readable.
pub struct HistorySession {
    path: PathBuf,
    seen: HashSet<String>,
    recent: Vec<Transcript>,
    fragment: Vec<u8>,
    offset: u64,
    identity: Option<FileIdentity>,
    baseline_done: bool,
    discard_until_newline: bool,
    checkpoint: Vec<u8>,
}

impl HistorySession {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            seen: HashSet::new(),
            recent: Vec::new(),
            fragment: Vec::new(),
            offset: 0,
            identity: None,
            baseline_done: false,
            discard_until_newline: false,
            checkpoint: Vec::new(),
        }
    }

    /// Up to ten records, newest first.
    pub fn recent(&self) -> &[Transcript] {
        &self.recent
    }

    pub fn poll(&mut self) -> io::Result<HistoryBatch> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    self.baseline_done = true;
                    self.reset();
                }
                return Err(err);
            }
        };
        let identity = file_identity(&file)?;
        let len = file.metadata()?.len();
        if self.identity != Some(identity) || len < self.offset || !self.checkpoint_matches(&mut file)? {
            self.reset();
        }
        self.identity = Some(identity);

        let mut batch = HistoryBatch::default();
        file.seek(SeekFrom::Start(self.offset))?;
        // Read only up to the length seen above, so a writer appending meanwhile
        // cannot keep this loop going.
        let mut remaining = len - self.offset;
        let mut buffer = vec![0u8; 64 * 1024];
        while remaining > 0 {
            let want = buffer.len().min(remaining as usize);
            let count = file.read(&mut buffer[..want])?;
            if count == 0 {
                break;
            }
            self.offset += count as u64;
            remaining -= count as u64;
            for &byte in &buffer[..count] {
                if byte == b'\n' {
                    if !self.discard_until_newline {
                        self.parse_fragment(&mut batch, true);
                    }
                    self.fragment.clear();
                    self.discard_until_newline = false;
                } else if !self.discard_until_newline {
                    self.fragment.push(byte);
                    if self.fragment.len() > MAX_LINE {
                        self.fragment = Vec::new();
                        self.discard_until_newline = true;
                        batch.invalid_lines += 1;
                    }
                }
            }
        }
        // A complete final object is accepted without its newline; anything else stays
        // buffered (split UTF-8, half-written JSON) for the next poll.
        if !self.fragment.is_empty() && self.parse_fragment(&mut batch, false) {
            self.fragment.clear();
        }

        // Records parsed above are already in `seen`, so a failure here must not drop
        // the batch. A file truncated meanwhile simply gets an empty checkpoint; the
        // size check on the next poll then starts over.
        let size = CHECKPOINT.min(self.offset);
        let mut checkpoint = vec![0u8; size as usize];
        let read = file.seek(SeekFrom::Start(self.offset - size)).and_then(|_| file.read_exact(&mut checkpoint));
        self.checkpoint = if read.is_ok() { checkpoint } else { Vec::new() };

        if !self.baseline_done {
            self.baseline_done = true;
            batch.new_records.clear();
        }
        Ok(batch)
    }

    /// Returns whether the fragment was consumed.
    fn parse_fragment(&mut self, batch: &mut HistoryBatch, complete: bool) -> bool {
        let mut bytes = self.fragment.as_slice();
        // Allowed on every line: the producer may rewrite the file with a fresh BOM.
        if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
            bytes = rest;
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return true;
        }
        let value: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(_) => {
                if complete {
                    batch.invalid_lines += 1;
                }
                return false;
            }
        };
        let field = |name: &str| {
            value.get(name).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty())
        };
        let (Some(id), Some(text)) = (field("id"), field("text")) else {
            if complete {
                batch.invalid_lines += 1;
            }
            return complete;
        };
        let record = Transcript::new(id, text);
        if self.seen.insert(record.id.clone()) {
            self.recent.insert(0, record.clone());
            self.recent.truncate(RECENT);
            if self.baseline_done {
                batch.new_records.push(record);
            }
        }
        true
    }

    fn checkpoint_matches(&self, file: &mut File) -> io::Result<bool> {
        if self.checkpoint.is_empty() {
            return Ok(true);
        }
        file.seek(SeekFrom::Start(self.offset - self.checkpoint.len() as u64))?;
        let mut current = vec![0u8; self.checkpoint.len()];
        match file.read_exact(&mut current) {
            Ok(()) => Ok(current == self.checkpoint),
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.fragment.clear();
        self.identity = None;
        self.discard_until_newline = false;
        self.checkpoint.clear();
    }
}
