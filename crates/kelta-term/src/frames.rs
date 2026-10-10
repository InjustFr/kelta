//! Terminal channel frame codec (ARCHITECTURE §6.1).
//!
//! First byte = tag, little-endian payloads:
//! `0x01` Data (raw PTY bytes), `0x02` Snapshot (ANSI repaint), `0x03` Exit (`i32`, `-1` = signal),
//! `0x04` Keyboard (`u8` kitty keyboard flags).

use kelta_proto::term::{FRAME_DATA, FRAME_EXIT, FRAME_KEYBOARD, FRAME_SNAPSHOT};

/// A decoded frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frame<'a> {
    Data(&'a [u8]),
    Snapshot(&'a [u8]),
    Exit(i32),
    Keyboard(u8),
}

/// `0x01` + raw bytes.
pub fn data(bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes.len() + 1);
    v.push(FRAME_DATA);
    v.extend_from_slice(bytes);
    v
}

/// A buffer that already starts with the Snapshot tag; the encoder appends the repaint to it,
/// so the frame is built without copying the payload.
pub fn snapshot_buffer(capacity: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(capacity + 1);
    v.push(FRAME_SNAPSHOT);
    v
}

/// `0x02` + payload.
pub fn snapshot(payload: &[u8]) -> Vec<u8> {
    let mut v = snapshot_buffer(payload.len());
    v.extend_from_slice(payload);
    v
}

/// `0x03` + `i32` LE (`-1` = killed by a signal).
pub fn exit(code: i32) -> Vec<u8> {
    let mut v = Vec::with_capacity(5);
    v.push(FRAME_EXIT);
    v.extend_from_slice(&code.to_le_bytes());
    v
}

/// `0x04` + kitty keyboard flags.
pub fn keyboard(flags: u8) -> Vec<u8> {
    vec![FRAME_KEYBOARD, flags]
}

/// The payload length a view acknowledges for this frame (Data and Snapshot only).
pub fn ack_len(frame: &[u8]) -> u32 {
    match frame.first() {
        Some(&FRAME_DATA) | Some(&FRAME_SNAPSHOT) => u32::try_from(frame.len() - 1).unwrap_or(u32::MAX),
        _ => 0,
    }
}

/// Decode a frame; `None` for an empty buffer, an unknown tag or a malformed Exit payload.
pub fn decode(frame: &[u8]) -> Option<Frame<'_>> {
    let (tag, payload) = frame.split_first()?;
    match *tag {
        FRAME_DATA => Some(Frame::Data(payload)),
        FRAME_SNAPSHOT => Some(Frame::Snapshot(payload)),
        FRAME_EXIT => {
            let bytes: [u8; 4] = payload.try_into().ok()?;
            Some(Frame::Exit(i32::from_le_bytes(bytes)))
        }
        FRAME_KEYBOARD => match payload {
            [flags] => Some(Frame::Keyboard(*flags)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_tag() {
        assert_eq!(decode(&data(b"hello")), Some(Frame::Data(b"hello")));
        assert_eq!(decode(&snapshot(b"\x1b[H")), Some(Frame::Snapshot(b"\x1b[H")));
        assert_eq!(decode(&exit(0)), Some(Frame::Exit(0)));
        assert_eq!(decode(&exit(-1)), Some(Frame::Exit(-1)));
        assert_eq!(decode(&exit(i32::MAX)), Some(Frame::Exit(i32::MAX)));
        assert_eq!(decode(&keyboard(31)), Some(Frame::Keyboard(31)));
    }

    #[test]
    fn tags_and_layout_match_the_contract() {
        assert_eq!(data(b"ab"), vec![0x01, b'a', b'b']);
        assert_eq!(snapshot(b"x"), vec![0x02, b'x']);
        assert_eq!(exit(3), vec![0x03, 3, 0, 0, 0]);
        assert_eq!(exit(-1), vec![0x03, 0xff, 0xff, 0xff, 0xff]);
        // Identical to the kelta-proto helpers.
        assert_eq!(data(b"zz"), kelta_proto::term::encode_frame(FRAME_DATA, b"zz"));
        assert_eq!(exit(7), kelta_proto::term::exit_frame(7));
    }

    #[test]
    fn empty_payloads_and_malformed_frames() {
        assert_eq!(decode(&data(b"")), Some(Frame::Data(b"")));
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[0x03, 1, 2]), None);
        assert_eq!(decode(&[0x09, 1]), None);
        assert_eq!(decode(&[0x04]), None);
    }

    #[test]
    fn ack_length_counts_payload_only() {
        assert_eq!(ack_len(&data(&[0u8; 100])), 100);
        assert_eq!(ack_len(&snapshot(b"abc")), 3);
        assert_eq!(ack_len(&exit(1)), 0);
        assert_eq!(ack_len(&keyboard(1)), 0);
        assert_eq!(ack_len(&[]), 0);
        let mut buf = snapshot_buffer(4);
        buf.extend_from_slice(b"abcd");
        assert_eq!(decode(&buf), Some(Frame::Snapshot(b"abcd")));
    }
}
