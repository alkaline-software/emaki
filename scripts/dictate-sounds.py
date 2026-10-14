#!/usr/bin/env python3
"""Writes the dictation's two sounds into crates/emaki-app/assets/sounds:
a soft chime of two notes rising as listening starts, and the same two
falling as it stops. Each note is a sine with a little of its octave and
twelfth in it, struck and let ring down; the second begins before the
first has died. Nothing but the standard library.

    python3 scripts/dictate-sounds.py
"""
import math, os, struct, wave

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "emaki-app", "assets", "sounds")


def note(freq, start, length, peak):
    """(start sample, samples) of one struck note."""
    n = int(length * RATE)
    out = []
    for i in range(n):
        t = i / RATE
        attack = min(1.0, t / 0.008)
        ring = math.exp(-t / 0.11)
        tail = min(1.0, (n - i) / (0.03 * RATE))
        tone = math.sin(2 * math.pi * freq * t) + 0.22 * math.sin(4 * math.pi * freq * t) + 0.06 * math.sin(6 * math.pi * freq * t)
        out.append(peak * attack * ring * tail * tone / 1.28)
    return int(start * RATE), out


def chime(freqs, name):
    notes = [note(f, k * 0.11, 0.34, 0.42 if k == 0 else 0.36) for k, f in enumerate(freqs)]
    total = max(s + len(x) for s, x in notes)
    mix = [0.0] * total
    for s, x in notes:
        for i, v in enumerate(x):
            mix[s + i] += v
    with wave.open(os.path.join(OUT, name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", max(-32767, min(32767, int(v * 32767)))) for v in mix))


os.makedirs(OUT, exist_ok=True)
# G5 then C6, a fourth up: listening. The same down: done.
chime([783.99, 1046.50], "dictate-start.wav")
chime([1046.50, 783.99], "dictate-stop.wav")
