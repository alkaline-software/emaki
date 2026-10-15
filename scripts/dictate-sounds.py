#!/usr/bin/env python3
"""Writes the dictation's two sounds into crates/emaki-app/assets/sounds:
two low, soft notes rising as listening starts, and the same two falling
as it stops. Each note is a sine with a trace of its octave, swelling in
over a few hundredths of a second (a strike read as an alert) and dying
away soon; the second begins before the first has gone. The first ones,
a fifth higher with more overtones and struck, sounded like a warning.
Nothing but the standard library.

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
        attack = 0.5 - 0.5 * math.cos(math.pi * min(1.0, t / 0.025))
        ring = math.exp(-t / 0.09)
        tail = min(1.0, (n - i) / (0.04 * RATE))
        tone = math.sin(2 * math.pi * freq * t) + 0.05 * math.sin(4 * math.pi * freq * t)
        out.append(peak * attack * ring * tail * tone / 1.05)
    return int(start * RATE), out


def chime(freqs, name):
    notes = [note(f, k * 0.10, 0.30, 0.30 if k == 0 else 0.26) for k, f in enumerate(freqs)]
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
# A4 then D5, a fourth up: listening. The same down: done.
chime([440.00, 587.33], "dictate-start.wav")
chime([587.33, 440.00], "dictate-stop.wav")
