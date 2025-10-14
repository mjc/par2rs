# How `par2cmdline` Parses PAR2 Files

`par2cmdline` is a command-line utility for creating and using PAR2 files, which are used for error detection and correction in data files. This document provides an overview of how `par2cmdline` parses PAR2 files.

## Overview of PAR2 Files

PAR2 files are structured binary files that contain metadata and recovery data for a set of files. They are used to verify the integrity of files and recover missing or corrupted data. The main components of a PAR2 file include:

- **PAR2 Header**: Contains metadata about the PAR2 file, such as magic bytes, length, MD5 checksum, set ID, and type of packet.
- **Main Packet**: Describes the main properties of the PAR2 file, including the number of files and recovery blocks.
- **File Description Packets**: Provide details about the files being protected, such as their names and sizes.
- **Input File Slice Checksum Packets**: Contain checksums for slices of the input files to verify their integrity.
- **Recovery Slice Packets**: Contain recovery data that can be used to reconstruct missing or corrupted slices of the input files.
- **Creator Packet**: Contains information about the software that created the PAR2 file.

## Parsing Process

The parsing process in `par2cmdline` involves reading and interpreting the binary data in a PAR2 file. The steps are as follows:

1. **Open the File**: The PAR2 file is opened for reading.
2. **Read the PAR2 Header**: The header is read and parsed to extract metadata about the file.
3. **Read the Main Packet**: The main packet is read to determine the number of files and recovery blocks.
4. **Read File Description Packets**: For each file described in the main packet, a file description packet is read and parsed.
5. **Read Input File Slice Checksum Packets**: For each file slice, a checksum packet is read and parsed to verify the integrity of the slices.
6. **Read Recovery Slice Packets**: For each recovery block, a recovery slice packet is read and parsed to extract recovery data.
7. **Read the Creator Packet**: The creator packet is read to identify the software that created the PAR2 file.

## Example Code

Below is an example of how the parsing process might be implemented in Rust:

```rust
if file_path.exists() {
    let mut file = fs::File::open(file_path).expect("Failed to open file");
    let header: Par2Header = file.read_le().expect("Failed to read Par2Header");
    println!("Parsed Par2Header: {:?}", header);

    let main_packet: MainPacket = file.read_le().expect("Failed to read MainPacket");
    println!("Parsed MainPacket: {:?}", main_packet);

    for _ in 0..main_packet.file_count {
        let file_description: FileDescriptionPacket = file.read_le().expect("Failed to read FileDescriptionPacket");
        // println!("Parsed FileDescriptionPacket: {:?}", file_description);
    }

    for _ in 0..main_packet.file_count {
        let input_file_slice_checksum: InputFileSliceChecksumPacket = file.read_le().expect("Failed to read InputFileSliceChecksumPacket");
        println!("Parsed InputFileSliceChecksumPacket: {:?}", input_file_slice_checksum);
    }

    for _ in 0..header.recovery_block_count {
        let recovery_slice: RecoverySlicePacket = file.read_le().expect("Failed to read RecoverySlicePacket");
        println!("Parsed RecoverySlicePacket: {:?}", recovery_slice);
    }

    let creator_packet: CreatorPacket = file.read_le().expect("Failed to read CreatorPacket");
    println!("Parsed CreatorPacket: {:?}", creator_packet);
} else {
    eprintln!("File does not exist: {}", input_file);
}
```

## Conclusion

`par2cmdline` provides a robust mechanism for parsing PAR2 files, enabling users to verify and recover data efficiently. By understanding the structure of PAR2 files and the parsing process, developers can implement similar functionality in their own applications.
