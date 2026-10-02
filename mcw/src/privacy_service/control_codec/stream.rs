//! Incremental grammar owner. Each new input octet is examined once; completed
//! lines are projected once. No accumulated reply prefix is replayed or rescanned.

use super::{Error, MAX_INPUT, MAX_LINE, MAX_LINES, Reply, Scan, ascii, status};

pub const MAX_BUFFER_BYTES: usize = MAX_INPUT + 2 * MAX_LINE + MAX_LINES * size_of::<String>();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Reply,
    Line,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Work {
    pub examined: usize,
    pub projected: usize,
    pub peak_buffer_bytes: usize,
}

#[derive(Clone, Copy)]
enum Phase {
    First,
    Minus,
    Plus,
    Terminal,
    Done,
}

/// One reply/line, owned entirely by Rust. It retains only the current raw line
/// plus already projected lines. The service removes it on completion or error.
pub struct Decoder {
    kind: Kind,
    phase: Phase,
    pending_cr: bool,
    current: Vec<u8>,
    lines: Vec<String>,
    code: i32,
    retained_line_capacity: usize,
    work: Work,
}

impl Decoder {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            phase: Phase::First,
            pending_cr: false,
            current: Vec::new(),
            lines: Vec::new(),
            code: 0,
            retained_line_capacity: 0,
            work: Work::default(),
        }
    }

    pub fn work(&self) -> Work {
        self.work
    }

    fn account_buffers(&mut self, temporary: usize) {
        self.work.peak_buffer_bytes = self.work.peak_buffer_bytes.max(
            self.current.capacity()
                + self.lines.capacity() * size_of::<String>()
                + self.retained_line_capacity
                + temporary,
        );
    }

    fn append(&mut self, byte: u8) -> Result<(), Error> {
        if self.current.len() == MAX_LINE {
            return Err(Error::Limit);
        }
        self.current.push(byte);
        self.account_buffers(0);
        Ok(())
    }

    fn push(&mut self, line: String) -> Result<(), Error> {
        if self.lines.len() == MAX_LINES {
            return Err(Error::Limit);
        }
        self.retained_line_capacity += line.capacity();
        self.lines.push(line);
        self.account_buffers(0);
        Ok(())
    }

    fn finish_line(&mut self) -> Result<bool, Error> {
        self.work.projected += self.current.len();
        let mut line = ascii(&self.current);
        self.current.clear();
        self.account_buffers(line.capacity());
        if self.kind == Kind::Line {
            self.push(line)?;
            return Ok(true);
        }
        match self.phase {
            Phase::First => {
                if line.len() < 3 {
                    return Err(Error::MissingStatus);
                }
                let prefix = line.as_bytes()[..3].try_into().unwrap();
                self.code = status(&prefix).ok_or(Error::InvalidStatus(prefix))?;
                line.drain(..3);
                let Some(&separator) = line.as_bytes().first() else {
                    self.push(line)?;
                    return Ok(true);
                };
                match separator {
                    b'-' => self.phase = Phase::Minus,
                    b'+' => self.phase = Phase::Plus,
                    _ => {
                        if separator == b' ' {
                            line.drain(..1);
                        }
                        self.push(line)?;
                        return Ok(true);
                    }
                }
                line.drain(..1);
                self.push(line)?;
            }
            Phase::Minus => {
                if line.is_empty() {
                    return Ok(false);
                }
                if line.len() > 3 && line.as_bytes()[3] == b' ' {
                    self.push(line)?;
                    return Ok(true);
                }
                if line.len() > 3 {
                    line.drain(..4);
                }
                self.push(line)?;
            }
            Phase::Plus => {
                if line.is_empty() {
                    return Ok(false);
                }
                if line == "." {
                    self.phase = Phase::Terminal;
                }
                self.push(line)?;
            }
            Phase::Terminal => {
                self.push(line)?;
                return Ok(true);
            }
            Phase::Done => return Err(Error::Limit),
        }
        Ok(false)
    }

    /// Consumption is relative to this new chunk, never a previously fed prefix.
    /// EOF errors are classified using the retained grammar/partial-line state.
    pub fn feed(&mut self, bytes: &[u8], eof: bool) -> Result<Scan<Reply>, Error> {
        if matches!(self.phase, Phase::Done) {
            return Err(Error::Limit);
        }
        for (index, &byte) in bytes.iter().enumerate() {
            if self.work.examined == MAX_INPUT {
                return Err(Error::Limit);
            }
            self.work.examined += 1;
            if self.pending_cr {
                self.pending_cr = false;
                if byte == b'\n' {
                    if self.finish_line()? {
                        self.phase = Phase::Done;
                        return Ok(Scan::Complete {
                            consumed: index + 1,
                            value: Reply {
                                status: self.code,
                                lines: std::mem::take(&mut self.lines),
                            },
                        });
                    }
                    continue;
                }
                self.append(b'\r')?;
            }
            if byte == b'\r' {
                self.pending_cr = true;
            } else {
                self.append(byte)?;
            }
        }
        if self.work.examined == MAX_INPUT {
            return Err(Error::Limit);
        }
        if eof {
            let incomplete = self.pending_cr || !self.current.is_empty();
            return Err(
                if self.kind == Kind::Reply && matches!(self.phase, Phase::First) {
                    Error::NoReplyLine { incomplete }
                } else if incomplete {
                    Error::IncompleteLine
                } else {
                    Error::NoMoreData
                },
            );
        }
        Ok(Scan::NeedMore)
    }
}
