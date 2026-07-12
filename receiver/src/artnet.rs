//! Minimal Art-Net packet decode.
//!
//! Byte layout and constants are taken from the authoritative implementations
//! in `artnet-tester`: the Rust sender (`src/artnet/core.rs`) and the C++
//! receiver (`VolumetricDisplay::listenArtNet`). We only need the two opcodes
//! the install uses: ArtDmx (0x5000) and ArtSync (0x5200).

/// `"Art-Net\0"` — the 8-byte ID that prefixes every packet.
pub const ARTNET_ID: &[u8; 8] = b"Art-Net\0";
pub const OP_DMX: u16 = 0x5000;
pub const OP_SYNC: u16 = 0x5200;

/// Smallest valid ArtDmx packet: 8 (id) + 2 (opcode) + 2 (protver) + 1 (seq)
/// + 1 (physical) + 2 (universe) + 2 (length) = 18 header bytes.
pub const ARTDMX_HEADER_LEN: usize = 18;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet<'a> {
    /// ArtDmx: one universe of channel data.
    Dmx { universe: u16, data: &'a [u8] },
    /// ArtSync: latch/present the frame accumulated so far.
    Sync,
}

/// Parse one received datagram. Returns `None` for anything that isn't a
/// well-formed ArtDmx/ArtSync packet (mirroring the C++ receiver, which simply
/// `continue`s on non-Art-Net traffic).
pub fn parse(buf: &[u8]) -> Option<Packet<'_>> {
    if buf.len() < 10 || &buf[0..8] != ARTNET_ID {
        return None;
    }
    // Opcode is little-endian (see core.rs `0x5000u16.to_le_bytes()`).
    let opcode = u16::from_le_bytes([buf[8], buf[9]]);
    match opcode {
        OP_SYNC => Some(Packet::Sync),
        OP_DMX => {
            if buf.len() < ARTDMX_HEADER_LEN {
                return None;
            }
            // Universe: little-endian. Length: big-endian. Clamp to 512 like the
            // C++ receiver, and never read past the datagram.
            let universe = u16::from_le_bytes([buf[14], buf[15]]);
            let dmx_length = u16::from_be_bytes([buf[16], buf[17]]).min(512) as usize;
            let avail = buf.len() - ARTDMX_HEADER_LEN;
            let data = &buf[ARTDMX_HEADER_LEN..ARTDMX_HEADER_LEN + dmx_length.min(avail)];
            Some(Packet::Dmx { universe, data })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an ArtDmx packet exactly like `artnet-tester`'s sender.
    fn dmx_packet(universe: u16, data: &[u8]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(ARTNET_ID);
        p.extend_from_slice(&OP_DMX.to_le_bytes());
        p.extend_from_slice(&14u16.to_be_bytes()); // protocol version
        p.push(0); // sequence
        p.push(0); // physical
        p.extend_from_slice(&universe.to_le_bytes());
        p.extend_from_slice(&(data.len() as u16).to_be_bytes());
        p.extend_from_slice(data);
        p
    }

    fn sync_packet() -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(ARTNET_ID);
        p.extend_from_slice(&OP_SYNC.to_le_bytes());
        p.extend_from_slice(&14u16.to_be_bytes());
        p.push(0);
        p.push(0);
        p
    }

    #[test]
    fn decodes_dmx() {
        let body = [1u8, 2, 3, 4, 5, 6];
        let pkt = dmx_packet(7, &body);
        match parse(&pkt).unwrap() {
            Packet::Dmx { universe, data } => {
                assert_eq!(universe, 7);
                assert_eq!(data, &body);
            }
            _ => panic!("expected Dmx"),
        }
    }

    #[test]
    fn decodes_sync() {
        assert_eq!(parse(&sync_packet()).unwrap(), Packet::Sync);
    }

    #[test]
    fn rejects_non_artnet() {
        assert!(parse(b"NOPE\0\0\0\0\x00\x50").is_none());
        assert!(parse(b"short").is_none());
    }

    #[test]
    fn truncated_dmx_clamps_to_available_bytes() {
        // Claim 510 bytes of data but only provide 6.
        let mut pkt = dmx_packet(0, &[9, 8, 7, 6, 5, 4]);
        pkt[16] = 0x01; // length high byte -> 0x01FE = 510
        pkt[17] = 0xFE;
        match parse(&pkt).unwrap() {
            Packet::Dmx { data, .. } => assert_eq!(data.len(), 6),
            _ => panic!("expected Dmx"),
        }
    }
}
