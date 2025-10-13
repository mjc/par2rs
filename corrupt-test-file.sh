#!/usr/bin/env zsh
# Usage: corrupt-test-file.sh <directory>
  if [ $# -eq 0 ]; then
      echo "Usage: $0 <directory>"
      echo "Corrupts a random 4KB block in a random file (excluding .par2 files)"
      exit 1
  fi
  
  cd "$1" || exit 1

# Pick a random file (excluding .par2 files)
file=$(ls | grep -v '\.par2$' | shuf -n 1)

# Make a backup first!
cp "$file" "${file}.backup"

# Corrupt a random 4KB block
filesize=$(stat -c%s "$file")
offset=$(( (RANDOM * RANDOM) % (filesize - 4096) ))
dd if=/dev/urandom of="$file" bs=1 count=4096 seek=$offset conv=notrunc
echo "Corrupted file: $file"
echo "Corrupted 4KB at offset: $offset"