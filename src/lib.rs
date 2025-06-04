pub mod args;
pub mod packets;
pub mod verify;

pub use args::parse_args;
pub use packets::*; // Add this line to import all public items from packets module

pub fn parse_packets<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Vec<Packet> {
    let mut packets = Vec::new();

    while let Some(packet) = Packet::parse(reader, &[]) {
        packets.push(packet);
    }

    packets
}
