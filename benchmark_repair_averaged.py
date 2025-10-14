#!/usr/bin/env python3
"""
Benchmark par2rs repair performance against par2cmdline with averaging
Runs 10 iterations and computes averages
"""

import subprocess
import tempfile
import shutil
import time
import hashlib
import sys
from pathlib import Path

# Configuration
ITERATIONS = 10
PROJECT_ROOT = Path(__file__).parent
PAR2RS = PROJECT_ROOT / "target/release/par2repair"
PAR2CMDLINE = "par2"

# Colors
BLUE = '\033[0;34m'
GREEN = '\033[0;32m'
YELLOW = '\033[1;33m'
RED = '\033[0;31m'
NC = '\033[0m'

def print_color(color, text):
    print(f"{color}{text}{NC}")

def get_md5(filepath):
    with open(filepath, 'rb') as f:
        return hashlib.md5(f.read()).hexdigest()

def main():
    print_color(BLUE, "=" * 40)
    print_color(BLUE, "PAR2 Repair Benchmark (Averaged)")
    print_color(BLUE, "Testing 100MB file repair")
    print_color(BLUE, f"Iterations: {ITERATIONS}")
    print_color(BLUE, "=" * 40)
    print()

    # Build par2rs
    print_color(YELLOW, "Building par2rs...")
    subprocess.run(["cargo", "build", "--release"], cwd=PROJECT_ROOT, 
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    print()

    # Create test file
    with tempfile.TemporaryDirectory() as temp_dir:
        temp_path = Path(temp_dir)
        testfile = temp_path / "testfile_100mb"
        
        print_color(YELLOW, f"Creating 100MB test file in {temp_dir}...")
        subprocess.run(["dd", "if=/dev/urandom", f"of={testfile}", 
                       "bs=1M", "count=100"], 
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        print()

        # Create PAR2 files
        print_color(YELLOW, "Creating PAR2 files with 5% redundancy...")
        subprocess.run([PAR2CMDLINE, "c", "-r5", 
                       f"{testfile}.par2", str(testfile)],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        print()

        # Get original MD5
        md5_original = get_md5(testfile)
        print(f"Original MD5: {md5_original}")
        print()

        # Arrays for times
        par2cmd_times = []
        par2rs_times = []

        for i in range(1, ITERATIONS + 1):
            print_color(GREEN, f"=== Iteration {i}/{ITERATIONS} ===")
            
            # Create working directory
            with tempfile.TemporaryDirectory() as iter_dir:
                iter_path = Path(iter_dir)
                iter_testfile = iter_path / "testfile_100mb"
                
                # Copy files
                shutil.copy(testfile, iter_testfile)
                for par2file in temp_path.glob("*.par2"):
                    shutil.copy(par2file, iter_path)
                
                # Corrupt file
                with open(iter_testfile, 'r+b') as f:
                    f.seek(50 * 1024 * 1024)  # 50MB offset
                    f.write(b'\x00' * (1024 * 1024))  # 1MB of zeros
                
                # Benchmark par2cmdline
                start = time.time()
                subprocess.run([PAR2CMDLINE, "r", f"{iter_testfile}.par2"],
                             cwd=iter_path,
                             stdout=subprocess.DEVNULL, 
                             stderr=subprocess.DEVNULL)
                par2cmd_time = time.time() - start
                par2cmd_times.append(par2cmd_time)
                
                # Verify par2cmdline
                md5_par2cmd = get_md5(iter_testfile)
                
                # Corrupt again for par2rs
                with open(iter_testfile, 'r+b') as f:
                    f.seek(50 * 1024 * 1024)
                    f.write(b'\x00' * (1024 * 1024))
                
                # Benchmark par2rs
                start = time.time()
                subprocess.run([str(PAR2RS), f"{iter_testfile}.par2"],
                             cwd=iter_path,
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
                par2rs_time = time.time() - start
                par2rs_times.append(par2rs_time)
                
                # Verify par2rs
                md5_par2rs = get_md5(iter_testfile)
                
                print(f"  par2cmdline: {par2cmd_time:.3f}s")
                print(f"  par2rs:      {par2rs_time:.3f}s")
                
                # Verify correctness
                if md5_par2cmd != md5_original or md5_par2rs != md5_original:
                    print_color(RED, f"✗ Repair verification failed in iteration {i}!")
                    sys.exit(1)
            
            print()

        # Calculate statistics
        par2cmd_avg = sum(par2cmd_times) / len(par2cmd_times)
        par2rs_avg = sum(par2rs_times) / len(par2rs_times)
        par2cmd_min = min(par2cmd_times)
        par2cmd_max = max(par2cmd_times)
        par2rs_min = min(par2rs_times)
        par2rs_max = max(par2rs_times)
        speedup = par2cmd_avg / par2rs_avg

        # Print results
        print_color(BLUE, "=" * 40)
        print_color(BLUE, f"Results ({ITERATIONS} iterations)")
        print_color(BLUE, "=" * 40)
        print()
        print_color(YELLOW, "par2cmdline:")
        print(f"  Average: {par2cmd_avg:.3f}s")
        print(f"  Min:     {par2cmd_min:.3f}s")
        print(f"  Max:     {par2cmd_max:.3f}s")
        print()
        print_color(YELLOW, "par2rs:")
        print(f"  Average: {par2rs_avg:.3f}s")
        print(f"  Min:     {par2rs_min:.3f}s")
        print(f"  Max:     {par2rs_max:.3f}s")
        print()
        print_color(GREEN, f"Speedup: {speedup:.2f}x")
        print()

        # Individual times
        print_color(YELLOW, "Individual times:")
        print("Iteration | par2cmdline | par2rs")
        print("----------|-------------|--------")
        for i, (p2c, p2r) in enumerate(zip(par2cmd_times, par2rs_times), 1):
            print(f"{i:9d} | {p2c:11.3f}s | {p2r:6.3f}s")
        print()

        print_color(GREEN, "✓ All repairs verified correct")

if __name__ == "__main__":
    main()
