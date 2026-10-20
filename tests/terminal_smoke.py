#!/usr/bin/env python3
"""Unix PTY integration checks. Run after cargo build --release.

Usage: python3 tests/terminal_smoke.py [path/to/limen]
No Python packages or graphical terminal emulator are required.
"""
import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

BINARY = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/release/limen").resolve())
MODES = [b"shaded", b"depth", b"silhouette", b"normal-x", b"normal-y", b"normal-z"]


class Viewer:
    def __init__(self, *args):
        self.master, self.slave = pty.openpty()
        self.resize(120, 40, 1200, 800)
        self.before = termios.tcgetattr(self.slave)
        self.process = subprocess.Popen(
            [BINARY, *args], stdin=self.slave, stdout=self.slave, stderr=self.slave
        )
        self.all_output = bytearray()

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=3)
        os.close(self.master)
        os.close(self.slave)

    def resize(self, cols, rows, pixel_width, pixel_height):
        self.cols, self.rows = cols, rows
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, pixel_width, pixel_height))

    def send(self, data):
        os.write(self.master, data)

    def collect(self, seconds=0.18):
        data = bytearray()
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if select.select([self.master], [], [], min(0.03, max(0, end - time.monotonic())))[0]:
                data.extend(os.read(self.master, 65536))
        self.all_output.extend(data)
        return bytes(data)

    def frames(self, seconds=0.18):
        result = []
        for fragment in self.collect(seconds).split(b"\x1b[1;1H")[1:]:
            rows = re.split(rb"\x1b\[\d+;1H", fragment)
            if len(rows) >= self.rows and all(len(row) >= self.cols for row in rows[:self.rows]):
                result.append(tuple(row[:self.cols] for row in rows[:self.rows]))
        assert result, "no complete frames"
        return result

    def last(self):
        return self.frames()[-1]

    def quit(self, key):
        self.send(key)
        self.collect(0.1)
        assert self.process.wait(timeout=3) == 0, self.all_output.decode(errors="replace")
        assert termios.tcgetattr(self.slave) == self.before, "raw mode not restored"
        assert b"\x1b[?1049l" in self.all_output, "alternate screen not restored"
        assert b"\x1b[?25h" in self.all_output, "cursor not restored"


with Viewer() as viewer:
    initial = viewer.frames(0.3)
    assert b"A0.500(auto)" in initial[-1][-1]
    assert any(row.strip() for row in initial[-1][:-1]), "no visible model"
    viewer.send(b" ")
    viewer.collect()
    paused = viewer.frames()
    assert len(paused) >= 2 and paused[0] == paused[-1], "pause did not freeze"
    for number, mode in enumerate(MODES, 1):
        viewer.send(str(number).encode())
        assert viewer.last()[-1].startswith(mode + b" paused"), mode
    viewer.send(b"2")
    depth = viewer.last()
    viewer.send(b".[")
    changed = viewer.last()
    assert depth[:-1] == changed[:-1], "tone changed debug visualization"
    assert depth[-1] != changed[-1], "tone controls not handled"
    viewer.send(b"3")
    silhouette = viewer.last()
    viewer.send(b"+")
    enlarged = viewer.last()
    assert sum(c != 32 for row in enlarged[:-1] for c in row) > sum(c != 32 for row in silhouette[:-1] for c in row), "zoom did not enlarge silhouette"
    viewer.send(b"\x1b[D\x1b[A")
    assert b"L-60/40" in viewer.last()[-1], "light arrows failed"
    viewer.send(b"0")
    reset = viewer.last()
    assert reset[-1].startswith(b"shaded paused z1.00 c1.20 g1.00 L-55/35")
    viewer.send(b"r")
    reset_angle = viewer.last()
    viewer.send(b" ")
    resumed = viewer.frames(0.4)
    assert len(set(frame[:-1] for frame in resumed)) > 1, "animation did not resume"
    viewer.resize(80, 24, 800, 600)
    resized = viewer.last()
    assert b"A0.400(auto)" in resized[-1], "resize aspect detection failed"
    viewer.resize(80, 24, 0, 0)
    assert b"A0.500(fallback)" in viewer.last()[-1]
    viewer.resize(80, 1, 800, 20)
    assert viewer.last()[0].startswith(b"shaded"), "single-row terminal failed"
    viewer.quit(b"q")
print("OK: pause, all modes, debug tone bypass, zoom, light, reset, resume, resize, auto/fallback aspect, one-row terminal, Q cleanup")

with Viewer("--cell-aspect", "0.6", "--view", "silhouette", "--contrast", "0.8") as viewer:
    assert b"A0.600(manual)" in viewer.last()[-1]
    viewer.resize(80, 24, 800, 600)
    assert b"A0.600(manual)" in viewer.last()[-1]
    viewer.send(b"1.0")
    reset = viewer.last()
    assert reset[-1].startswith(b"silhouette") and b"c0.80" in reset[-1], "reset did not restore CLI settings"
    viewer.quit(b"\x1b")
print("OK: manual aspect precedence, CLI reset, Esc cleanup")

with Viewer() as viewer:
    viewer.last()
    viewer.quit(b"\x03")
print("OK: Ctrl+C cleanup")
