use binread::BinReaderExt;
use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use par2rs::parse_args;
use par2rs::{MainPacket, PackedMainPacket, FileDescriptionPacket, RecoverySlicePacket, CreatorPacket, InputFileSliceChecksumPacket};
use par2rs::Packet;

fn parse_packets(file: &mut fs::File) -> Vec<Packet> {
    let mut packets = Vec::new();

    loop {
        let mut magic = [0u8; 8];
        if let Err(_) = file.read_exact(&mut magic) {
            break; // End of file or error
        }

        if &magic != b"PAR2\0PKT" {
            eprintln!("Invalid magic sequence: {:?}, stopping parsing.", magic);
            break;
        }

        file.seek(std::io::SeekFrom::Current(-8)).expect("Failed to rewind to the beginning of the packet");
        let mut header = [0u8; 64]; // Full 64-byte header including type_of_packet
        file.read_exact(&mut header).expect("Failed to read header");

        let type_of_packet = &header[48..64]; // Adjusted to correctly extract the last 16 bytes of the common header

        file.seek(std::io::SeekFrom::Current(-64)).expect("Failed to rewind to the beginning of the packet");

        match type_of_packet {
            b"PAR 2.0\0Main\0\0\0\0" => {
                let main_packet: MainPacket = file.read_le().expect("Failed to read MainPacket");
                packets.push(Packet::MainPacket(main_packet));
            }
            b"PAR 2.0\0PkdMain\0" => {
                let packed_main_packet: PackedMainPacket = file.read_le().expect("Failed to read PackedMainPacket");
                packets.push(Packet::PackedMainPacket(packed_main_packet));
            }
            b"PAR 2.0\0FileDesc" => {
                let file_description: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
                packets.push(Packet::FileDescriptionPacket(file_description));
            }
            b"PAR 2.0\0RecvSlic" => {
                let recovery_slice: RecoverySlicePacket = file.read_le().expect("Failed to read RecoverySlicePacket");
                packets.push(Packet::RecoverySlicePacket(recovery_slice));
            }
            b"PAR 2.0\0Creator\0" => {
                let creator_packet: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
                packets.push(Packet::CreatorPacket(creator_packet));
            }
            b"PAR 2.0\0IFSC\0\0\0\0" => {
                let input_file_slice_checksum_packet: InputFileSliceChecksumPacket = file.read_le().expect("Failed to read InputFileSliceChecksumPacket");
                packets.push(Packet::InputFileSliceChecksumPacket(input_file_slice_checksum_packet));
            }
            _ => {
                eprintln!("Unknown packet type: {:?}", String::from_utf8_lossy(&type_of_packet));
                break;
            }
        }
    }

    packets
}

fn main() {
    let matches = parse_args();

    let input_file = matches.get_one::<String>("input").expect("Input file is required");
    let output_file = matches.get_one::<String>("output");

    println!("Input file: {}", input_file);
    if let Some(output) = output_file {
        println!("Output file: {}", output);
    }

    let file_path = Path::new(input_file);
    if file_path.exists() {
        let mut file = fs::File::open(file_path).expect("Failed to open file");
        let packets = parse_packets(&mut file);

        // Here you can do something with the packets, like processing or saving them
        println!("Parsed {} packets", packets.len());
    } else {
        eprintln!("File does not exist: {}", input_file);
    }
}
