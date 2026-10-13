"""Median fundamental frequency of WAV files (spec-narrate-test-strategy.md, QA assets; NFR-004).

Run it with the narrate venv and nothing else on the import path:

    <D>\\venv\\Scripts\\python.exe -I qa\\pitch\\median_f0.py <wav> [<wav> ...]

with NUMBA_CACHE_DIR=<D>\\numba-cache set, so librosa writes nothing outside the data directory.

For each WAV: librosa.load(path, sr=16000, mono=True), then
librosa.pyin(y, fmin=65.0, fmax=500.0, sr=16000, frame_length=1024) with the default hop (256).
Voiced frames are the frames where voiced_flag is true and f0 is finite. The median is the
numpy.median of f0 over the voiced frames. The value is printed unrounded (repr of the float), so
a threshold comparison such as 164.96 against 165.0 uses the exact median.

Output, one line per file on stdout:
    <path>\\tmedian_f0_hz=<repr of the float>\\tvoiced_frames=<n>\\tframes=<m>

Exit codes: 0 all files measured; 2 when any file has fewer than 100 voiced frames (that file
prints a diagnostic on stderr and no median line); 1 when a file cannot be read or when no file
was given.
"""

import sys

import librosa
import numpy

MIN_VOICED_FRAMES = 100


def measure(path):
    """Returns (median_f0_hz, voiced_frames, frames); the median is None below the minimum."""
    y, _ = librosa.load(path, sr=16000, mono=True)
    f0, voiced_flag, _voiced_probs = librosa.pyin(
        y, fmin=65.0, fmax=500.0, sr=16000, frame_length=1024
    )
    voiced = numpy.asarray(voiced_flag, dtype=bool) & numpy.isfinite(f0)
    voiced_frames = int(voiced.sum())
    frames = int(len(f0))
    if voiced_frames < MIN_VOICED_FRAMES:
        return None, voiced_frames, frames
    median = float(numpy.median(f0[voiced]))
    return median, voiced_frames, frames


def main(argv):
    if not argv:
        print("usage: median_f0.py <wav> [<wav> ...]", file=sys.stderr)
        return 1
    insufficient = False
    failed = False
    for path in argv:
        try:
            median, voiced_frames, frames = measure(path)
        except Exception as error:  # report every file, then fail
            print(f"{path}\terror={error!r}", file=sys.stderr)
            failed = True
            continue
        if median is None:
            print(
                f"{path}\tinsufficient voiced frames: voiced_frames={voiced_frames} "
                f"frames={frames} (need at least {MIN_VOICED_FRAMES})",
                file=sys.stderr,
            )
            insufficient = True
            continue
        print(f"{path}\tmedian_f0_hz={repr(median)}\tvoiced_frames={voiced_frames}\tframes={frames}")
        sys.stdout.flush()
    if insufficient:
        sys.exit(2)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
