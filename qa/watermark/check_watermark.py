"""PerTh watermark extraction for WAV files (spec-narrate-test-strategy.md, QA assets; FR-018).

Run it with the narrate venv and nothing else on the import path:

    <D>\\venv\\Scripts\\python.exe -I qa\\watermark\\check_watermark.py <wav> [<wav> ...]

with NUMBA_CACHE_DIR=<D>\\numba-cache set.

For each WAV: audio, sr = librosa.load(path, sr=None) (native rate), then
perth.PerthImplicitWatermarker().get_watermark(audio, sample_rate=sr). Output, one line per file
on stdout:
    <path>\\twatermark=<value with 3 decimals>

Expected values: >= 0.5 on every narrate output and < 0.5 on <D>\\voices\\female.wav (Kokoro
output, never watermarked). The extractor call follows the chatterbox README, not the research
notes. If the call raises or returns something that is not a single number, the script prints the
type it got on stderr and exits 1: QA reports that to the architect and does not rely on the
value (spec-narrate-test-strategy.md, Error handling).

Exit codes: 0 all files measured; 1 any failure or no file given.
"""

import sys

import librosa
import perth


def main(argv):
    if not argv:
        print("usage: check_watermark.py <wav> [<wav> ...]", file=sys.stderr)
        return 1
    watermarker = perth.PerthImplicitWatermarker()
    failed = False
    for path in argv:
        try:
            audio, sr = librosa.load(path, sr=None)
            value = watermarker.get_watermark(audio, sample_rate=sr)
            number = float(value)
        except Exception as error:  # report every file, then fail
            print(f"{path}\terror={error!r}", file=sys.stderr)
            failed = True
            continue
        print(f"{path}\twatermark={number:.3f}")
        sys.stdout.flush()
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
