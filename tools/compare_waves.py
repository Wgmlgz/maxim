"""Rough audio comparison for a confirmed transcript.

Usage: python3 tools/compare_waves.py REFERENCE.wav CANDIDATE.wav [...]
Requires NumPy and SciPy. Lower scores mean closer log-spectrum trajectories;
this is not a speech-quality or intelligibility score.
"""

import sys
import wave

import numpy as np
from scipy.signal import resample_poly, stft
from scipy.spatial.distance import cdist

RATE = 22050


def features(path):
    with wave.open(path) as source:
        data = np.frombuffer(source.readframes(source.getnframes()), dtype="<i2").astype(float)
        rate = source.getframerate()
        if source.getnchannels() != 1:
            raise ValueError(f"expected mono WAV: {path}")
    if rate != RATE:
        from math import gcd

        divisor = gcd(rate, RATE)
        data = resample_poly(data, RATE // divisor, rate // divisor)
    frequencies, _, spectrum = stft(data, RATE, nperseg=1024, noverlap=768)
    edges = np.linspace(0, 8000, 21)
    bands = np.stack(
        [
            np.abs(spectrum[(frequencies >= low) & (frequencies < high)]).mean(axis=0)
            for low, high in zip(edges[:-1], edges[1:])
        ],
        axis=1,
    )
    return np.log1p(bands), len(data) / RATE


def dtw(reference, candidate):
    distances = cdist(reference, candidate) / np.sqrt(reference.shape[1])
    previous = np.full(len(candidate) + 1, np.inf)
    previous[0] = 0
    for row in distances:
        current = np.full(len(candidate) + 1, np.inf)
        for column, value in enumerate(row, 1):
            current[column] = value + min(
                previous[column], current[column - 1], previous[column - 1]
            )
        previous = current
    return previous[-1] / (len(reference) + len(candidate))


if __name__ == "__main__":
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    reference, duration = features(sys.argv[1])
    print(f"reference: {duration:.3f} s")
    for path in sys.argv[2:]:
        candidate, duration = features(path)
        print(f"{path}: {duration:.3f} s, distance {dtw(reference, candidate):.4f}")
