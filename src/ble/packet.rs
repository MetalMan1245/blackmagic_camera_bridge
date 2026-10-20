//! Parser for Blackmagic Camera Control packets delivered as notifications
//! on the Incoming Camera Control GATT characteristic.
//!
//! Byte offsets below are derived from captured notification samples
//! (reverse-engineering, "Magic Button 4k" project):
//!   record ON:  FF 09 00 00 0A 01 01 02 02 00 40 00 02
//!   record OFF: FF 09 00 00 0A 01 01 02 00 00 40 00 02
//! They MUST be validated against the "Example Protocol Packets" section of
//! the official Blackmagic Camera Control PDF before use with real hardware.

pub const TRANSPORT_CATEGORY: u8 = 10;
pub const RECORD_PARAMETER: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum CameraPacket {
    Record { is_recording: bool },
    Other { category: u8, parameter: u8 },
    Malformed,
}

pub fn parse_packet(bytes: &[u8]) -> CameraPacket {
    if bytes.len() < 9 {
        return CameraPacket::Malformed;
    }
    let category = bytes[4];
    let parameter = bytes[6];
    if category == TRANSPORT_CATEGORY && parameter == RECORD_PARAMETER {
        return CameraPacket::Record {
            is_recording: bytes[8] != 0,
        };
    }
    CameraPacket::Other {
        category,
        parameter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REC_ON: &[u8] = &[0xFF, 9, 0, 0, 10, 1, 1, 2, 2, 0, 64, 0, 2];
    const REC_OFF: &[u8] = &[0xFF, 9, 0, 0, 10, 1, 1, 2, 0, 0, 64, 0, 2];

    #[test]
    fn parses_record_on_notification() {
        assert_eq!(
            parse_packet(REC_ON),
            CameraPacket::Record { is_recording: true }
        );
    }

    #[test]
    fn parses_record_off_notification() {
        assert_eq!(
            parse_packet(REC_OFF),
            CameraPacket::Record {
                is_recording: false
            }
        );
    }

    #[test]
    fn rejects_truncated_packet() {
        assert_eq!(parse_packet(&REC_ON[..8]), CameraPacket::Malformed);
    }

    #[test]
    fn unrelated_packet_is_other() {
        let mut bytes = REC_ON.to_vec();
        bytes[4] = 1;
        assert_eq!(
            parse_packet(&bytes),
            CameraPacket::Other {
                category: 1,
                parameter: 1
            }
        );
    }
}
