//! Recovery verification tests

use binrw::BinReaderExt;
use par2rs::packets::{InputFileSliceChecksumPacket, Packet, RecoverySlicePacket};
use std::fs::File;

mod recovery_slices {
    use super::*;

    #[test]
    fn multiple_fixtures() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/RecoverySlicePacket.par2") {
            if let Ok(packet) = file.read_le::<RecoverySlicePacket>() {
                let _ = packet.exponent;
                let _ = packet.recovery_data.len();
                let _ = packet.verify();
            }
        }
    }

    #[test]
    fn in_enum() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/RecoverySlicePacket.par2") {
            if let Ok(packet) = file.read_le::<RecoverySlicePacket>() {
                let enum_packet = Packet::RecoverySlice(packet);
                let _ = enum_packet.verify();
            }
        }
    }
}

mod input_file_checksums {
    use super::*;

    #[test]
    fn multiple_checksums() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/InputFileSliceChecksumPacket.par2")
        {
            if let Ok(packet) = file.read_le::<InputFileSliceChecksumPacket>() {
                let _ = packet.verify();
                let _ = packet.md5;
                let _ = packet.set_id;
            }
        }
    }

    #[test]
    fn in_enum() {
        if let Ok(mut file) = File::open("tests/fixtures/packets/InputFileSliceChecksumPacket.par2")
        {
            if let Ok(packet) = file.read_le::<InputFileSliceChecksumPacket>() {
                let enum_packet = Packet::InputFileSliceChecksum(packet);
                let _ = enum_packet.verify();
            }
        }
    }
}
