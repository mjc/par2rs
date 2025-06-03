# PAR2 Binary File Format Documentation

## Overview

The PAR2 binary file format is designed for error correction and data recovery. It is used to store redundancy data that can reconstruct missing or corrupted files. The format is structured to allow efficient verification and recovery of data.

## File Structure

A PAR2 binary file is composed of packets, each serving a specific purpose. The file begins with a header packet, followed by various other packets containing metadata, checksums, and recovery data.

### 1. Header Packet

The header packet is the first packet in the file and contains the following fields:

- **Magic Number**: A unique identifier for PAR2 files (`0x50415232` in hexadecimal, representing "PAR2").
- **Packet Length**: The total length of the packet, including the header.
- **Packet Type**: Specifies the type of packet (e.g., header, file description, recovery).
- **Set Identifier**: A unique identifier for the PAR2 volume set.

### 2. File Description Packet

The file description packet contains metadata about the original files being protected:

- **File Name**: The name of the file.
- **File Size**: The size of the file in bytes.
- **File Checksum**: A checksum to verify the integrity of the file.

### 3. Recovery Packet

The recovery packet contains parity data used for error correction:

- **Recovery Block Index**: The index of the recovery block within the set.
- **Recovery Data**: The actual parity data used to reconstruct missing or corrupted files.

### 4. Checksums

PAR2 files include checksums for verifying the integrity of the original files and recovery data:

- **MD5 Checksum**: Used to verify the integrity of individual packets.
- **SHA1 Checksum**: Used for additional verification of the entire file.

## Packet Format

Each packet in a PAR2 file follows a specific format:

1. **Packet Header**:
   - Magic Number (4 bytes)
   - Packet Length (4 bytes)
   - Packet Type (4 bytes)
   - Set Identifier (16 bytes)

2. **Packet Body**:
   - Contains data specific to the packet type (e.g., file metadata, recovery data).

## Recovery Process

The recovery process involves the following steps:

1. **Verification**:
   - The checksums in the PAR2 file are used to verify the integrity of the original files.

2. **Error Correction**:
   - Recovery packets are used to reconstruct missing or corrupted files using Reed-Solomon error correction algorithms.

## Advantages

- Efficient storage of redundancy data.
- Supports partial recovery of files.
- Allows verification of file integrity.

## Limitations

- Requires additional storage for PAR2 files.
- Recovery is limited by the number of recovery packets generated.

## References

- [PAR2 Specification](https://github.com/Parchive/par2cmdline)
- [Reed-Solomon Error Correction](https://en.wikipedia.org/wiki/Reed%E2%80%93Solomon_error_correction)

## Conclusion

The PAR2 binary file format is a robust solution for ensuring data integrity and recovering lost or corrupted files. Its structured design and use of error correction algorithms make it ideal for distributed systems and unreliable storage environments.
