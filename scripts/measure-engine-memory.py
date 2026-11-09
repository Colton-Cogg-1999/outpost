#!/usr/bin/env python3
"""Linux RSS sampler: compare the bundled engine's thread counts without a GUI."""
import pathlib
import subprocess
import sys
import threading
import time

engine = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'src-tauri/binaries/outpost-stockfish-x86_64-unknown-linux-gnu').resolve()
for threads in (1, 4):
    with subprocess.Popen([str(engine)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1) as process:
        ready = threading.Event()
        done = threading.Event()
        def read_output():
            for line in process.stdout:
                if line.startswith('readyok'):
                    ready.set()
                if line.startswith('bestmove'):
                    done.set()
        reader = threading.Thread(target=read_output, daemon=True)
        reader.start()
        def send(command):
            process.stdin.write(command + '\n')
            process.stdin.flush()
        send(f'uci\nsetoption name Threads value {threads}\nsetoption name Hash value 16\nisready')
        if not ready.wait(15):
            process.kill()
            raise RuntimeError('Engine did not become ready')
        send('position startpos\ngo movetime 3000')
        peak = 0
        deadline = time.monotonic() + 10
        while not done.is_set() and time.monotonic() < deadline:
            try:
                values = pathlib.Path(f'/proc/{process.pid}/status').read_text().splitlines()
                peak = max(peak, next(int(row.split()[1]) for row in values if row.startswith('VmRSS:')))
            except (FileNotFoundError, StopIteration):
                break
            time.sleep(0.02)
        send('quit')
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        print(f'{threads} thread(s), 16 MiB hash: sampled peak RSS {peak / 1024:.1f} MiB')
