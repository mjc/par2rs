# Parity Volume Set Specification 2.0

## Authors
- Michael Nahas
- Peter Clements
- Paul Nettle
- Ryan Gallagher

**Publication Date:** May 11th, 2003

---

## Revision History

| Revision | Date           | Notes                                      |
|----------|----------------|--------------------------------------------|
| 1.0      | October 14th, 2001 | Related specification, Inspiration         |
| 2.0      | May 11th, 2003    | New Specification, Initial publication and formatting |

---

## Abstract

Based on *Parity Volume Set Specification 1.0 [2001-10-14]* by Stefan Wehlus and others.

---

## Table of Contents

1. [Introduction](#introduction)
2. [Conventions](#conventions)
3. [Description](#description)
   - [Main packet](#main-packet)
   - [File Description packet](#file-description-packet)
   - [Input File Slice Checksum packet](#input-file-slice-checksum-packet)
   - [Recovery Slice packet](#recovery-slice-packet)
   - [Creator packet](#creator-packet)
4. [Conclusion](#conclusion)
5. Appendices
   - [Optional PAR 2.0 Packets](#optional-par-20-packets)
   - [How to Add an Application-Specific Packet Type](#how-to-add-an-application-specific-packet-type)
   - [GNU Free Documentation License](#gnu-free-documentation-license)

---

## Introduction

This document describes a file format for storing redundant data for a set of files. In operation, a user will select a set of files from which the redundant data is to be made. These are known as *input files* and the set of them is known as the *recovery set*. The user will provide these to a program which generates file(s) that match the specification in this document. The program is known as a *PAR 2.0 Client* or *client* for short, and the generated files are known as *PAR 2.0 files* or *PAR files*.

If the files in the recovery set ever get damaged (e.g., when they are transmitted or stored on a faulty disk), the client can read the damaged input files, read the (possibly damaged) PAR files, and regenerate the original input files. Of course, not all damages can be repaired, but many can.

---

## Conventions

- **Data Alignment:** The data is 4-byte aligned. Every field starts on an index in the file which is congruent to zero, modulus 4.
- **Integer Types:** All integers in this version of the spec are unsigned integers of either 4 or 8 bytes in length.
- **Strings:** Strings are not null-terminated. To make a string 4-byte aligned, 1 to 3 zero bytes may be appended.
- **File Identification:** Files are identified by a 16-byte value, calculated as an MD5 Hash of their name, length, and the MD5 Hash of their first 16kB.

---

## Description

A PAR 2.0 file consists of a sequence of "packets". A packet has a fixed-sized header and a variable-length body. The packet header contains a checksum for the packet - if the packet is damaged, the packet is ignored. The packet header also contains a packet-type. If the client does not understand the packet type, the packet is ignored.

### Main Packet

The main packet contains the slice size and the File IDs of all the files in the recovery set. The MD5 hash of the body of the main packet is used as the Recovery Set ID, which is included in the packet header of every packet for the set.

---

## Conclusion

That is the official spec. To make sure clients work similarly, the following client conventions should be followed:

- PAR 2.0 files should always end in `.par2`.
- If multiple PAR files are generated, they may either have a constant number of slices per file or exponentially increasing number of slices.
- All files must contain a creator packet.

---

## Appendices

### Optional PAR 2.0 Packets

Clients do not need to process these packets. They are included in this spec because many clients might want to implement the functionality and, if they did, it would be good if they were compatible with each other.

### How to Add an Application-Specific Packet Type

Say the author of "NewsPost" wanted to add his own packet type - one that identified the names of the Usenet messages in which the files are posted. That author can create his own packet type.

### GNU Free Documentation License

This document is licensed under the GNU Free Documentation License, Version 1.2 or any later version published by the Free Software Foundation.
