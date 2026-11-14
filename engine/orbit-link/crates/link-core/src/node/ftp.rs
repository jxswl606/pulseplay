//! FTP passive-mode fix.
//!
//! An FTP server reached through the forwarder sees a local connection and
//! announces a local address in its `227 Entering Passive Mode (h1,h2,h3,h4,p1,p2)`
//! reply. Remote clients cannot reach that address, so the reply is rewritten to
//! the device's Orbit Link address. The data port itself is forwarded like any
//! other port.

use std::net::Ipv4Addr;

/// FTP control ports whose replies are rewritten.
pub const FTP_PORTS: [u16; 3] = [21, 1337, 2121];

const MAX_LINE: usize = 4096;

pub struct PassiveRewriter {
    address: Ipv4Addr,
    partial: Vec<u8>,
}

impl PassiveRewriter {
    pub fn new(address: Ipv4Addr) -> Self {
        PassiveRewriter { address, partial: Vec::new() }
    }

    /// Feeds server output and returns the bytes to forward. Bytes after the
    /// last newline are held until the line is complete.
    pub fn feed(&mut self, data: &[u8], out: &mut Vec<u8>) {
        self.partial.extend_from_slice(data);
        while let Some(end) = self.partial.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            out.extend_from_slice(&self.rewrite(&line));
        }
        if self.partial.len() > MAX_LINE {
            out.append(&mut self.partial);
        }
    }

    /// Returns any held bytes (used when the server closes the connection).
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.partial);
    }

    fn rewrite(&self, line: &[u8]) -> Vec<u8> {
        if !line.starts_with(b"227") {
            return line.to_vec();
        }
        let text = String::from_utf8_lossy(line);
        let (Some(open), Some(close)) = (text.find('('), text.rfind(')')) else {
            return line.to_vec();
        };
        let fields: Vec<&str> = text[open + 1..close].split(',').map(str::trim).collect();
        if fields.len() != 6 || fields.iter().any(|f| f.parse::<u8>().is_err()) {
            return line.to_vec();
        }
        let [a, b, c, d] = self.address.octets();
        format!(
            "{}({a},{b},{c},{d},{},{}){}",
            &text[..open],
            fields[4],
            fields[5],
            &text[close + 1..]
        )
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_passive_reply_across_reads() {
        let mut rewriter = PassiveRewriter::new(Ipv4Addr::new(10, 77, 0, 2));
        let mut out = Vec::new();
        rewriter.feed(b"230 OK\r\n227 Entering Passive Mode (127,0,", &mut out);
        assert_eq!(out, b"230 OK\r\n");
        rewriter.feed(b"0,1,195,80).\r\n", &mut out);
        assert_eq!(out, b"230 OK\r\n227 Entering Passive Mode (10,77,0,2,195,80).\r\n");
    }

    #[test]
    fn leaves_other_replies_alone() {
        let mut rewriter = PassiveRewriter::new(Ipv4Addr::new(10, 77, 0, 2));
        let mut out = Vec::new();
        rewriter.feed(b"229 Entering Extended Passive Mode (|||50000|)\r\n227 bad (1,2)\r\n", &mut out);
        assert_eq!(out, b"229 Entering Extended Passive Mode (|||50000|)\r\n227 bad (1,2)\r\n");
    }
}
