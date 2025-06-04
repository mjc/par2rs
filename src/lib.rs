pub mod args;
pub mod packets;
pub mod verify;

pub use args::parse_args;
pub use packets::*; // Add this line to import all public items from packets module

pub fn parse_packets<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();
    let mut header = [0u8; 64];

    while let Ok(_) = reader.read_exact(&mut header) {
        let type_of_packet = &header[48..64];

        // Rewind the reader to the start of the packet
        reader.seek(std::io::SeekFrom::Current(-64)).expect("Failed to rewind reader");

        if let Some(packet) = Packet::from_bytes(type_of_packet, reader) {
            packets.push(packet);

            // Advance the reader by the packet length
            let packet_length = u64::from_le_bytes(header[0..8].try_into().unwrap());
            reader.seek(std::io::SeekFrom::Current(packet_length as i64 - 64)).expect("Failed to advance reader");
        } else {
            // Skip the rest of the packet if it cannot be parsed
            let packet_length = u64::from_le_bytes(header[0..8].try_into().unwrap());
            reader.seek(std::io::SeekFrom::Current(packet_length as i64 - 64)).expect("Failed to skip packet");
        }

        // Break the loop if the reader reaches the end of the file
        if reader.stream_position().unwrap() >= reader.seek(std::io::SeekFrom::End(0)).unwrap() {
            break;
        }
    }

    packets
}
