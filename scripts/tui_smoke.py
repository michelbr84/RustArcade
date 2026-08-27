#!/usr/bin/env python3
"""Drive the RustArcade TUI in a pseudo-terminal and check the launcher lifecycle.

Usage:
    RUSTARCADE_HOME=/tmp/ra python3 scripts/tui_smoke.py target/debug/rustarcade [game-id]

The home must contain at least one installed game (default: snakeshell). The script opens the
interface, searches for the game, opens its details, launches it, interrupts it with Ctrl+C and
Esc, and verifies that RustArcade comes back, reports the session, and restores the terminal
on quit. Linux/macOS only (uses the `pty` module).
"""
import fcntl
import os
import pty
import re
import select
import signal
import struct
import sys
import termios
import time

exe = sys.argv[1] if len(sys.argv) > 1 else "target/debug/rustarcade"
game = sys.argv[2] if len(sys.argv) > 2 else "snakeshell"
if not os.environ.get("RUSTARCADE_HOME"):
    sys.exit("set RUSTARCADE_HOME to a home with an installed game")

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["RUSTARCADE_OFFLINE"] = "1"
    os.execv(exe, [exe])
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 140, 0, 0))

captured = b""


def drain(seconds):
    global captured
    buf = b""
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            buf += data
    captured += buf
    return buf


def send(keys, wait=0.7):
    os.write(fd, keys)
    return drain(wait)


failures = 0


def check(name, haystack, needle):
    global failures
    ok = needle.encode() in haystack
    failures += 0 if ok else 1
    print(("PASS " if ok else "FAIL ") + name)


drain(2.0)
check("welcome dialog", captured, "Welcome to RustArcade")
send(b"\r", 0.8)
send(b"2", 0.8)
send(b"/", 0.4)
send(game.encode(), 0.8)
send(b"\r", 0.5)
send(b"\r", 0.8)
check("details screen", captured, "Repository")
before = len(captured)
send(b"\r", 3.0)
alt_leaves = captured.count(b"\x1b[?1049l")
check("interface suspended for the game", captured, "\x1b[?1049l")
send(b"\x03", 1.2)
send(b"\x1b", 1.2)
send(b"\x1b", 1.2)
drain(1.0)
after_game = captured[before:]
check("interface resumed after the game", after_game, "RUSTARCADE")
check("session recorded", after_game, "Played")
send(b"q", 1.5)
end = time.time() + 5
status = None
while time.time() < end:
    wpid, st = os.waitpid(pid, os.WNOHANG)
    if wpid:
        status = st
        break
    time.sleep(0.1)
if status is None:
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    print("FAIL quit: process did not exit")
    failures += 1
else:
    code = os.WEXITSTATUS(status) if os.WIFEXITED(status) else -os.WTERMSIG(status)
    check_ok = code == 0
    failures += 0 if check_ok else 1
    print(("PASS" if check_ok else "FAIL") + f" exit code {code}")
attrs = termios.tcgetattr(fd)
restored = bool(attrs[3] & termios.ECHO) and bool(attrs[3] & termios.ICANON)
failures += 0 if restored else 1
print(("PASS" if restored else "FAIL") + " terminal modes restored (ECHO + ICANON)")
drain(0.5)
left = captured.count(b"\x1b[?1049l") > alt_leaves
failures += 0 if left else 1
print(("PASS" if left else "FAIL") + " alternate screen left on quit")
sys.exit(1 if failures else 0)
