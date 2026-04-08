#!/usr/bin/env python3
"""
ML beat tracker using madmom's DBN models.

Outputs a single line of JSON:
  {"beats": [0.52, 1.04, ...], "downbeats": [0.52, 2.60, ...], "bpm": 120.3}

Beat and downbeat timestamps are in seconds from the start of the file.

Install:
  pip install madmom

Usage:
  python3 scripts/beat_tracker.py <audio_file>
"""

import sys
import json
import numpy as np


def track(audio_file: str) -> dict:
    from madmom.features.beats import RNNBeatProcessor, DBNBeatTrackingProcessor
    from madmom.features.downbeats import RNNDownBeatProcessor, DBNDownBeatTrackingProcessor

    # ── Beat tracking ────────────────────────────────────────────────────────
    beat_act = RNNBeatProcessor()(audio_file)
    beats = DBNBeatTrackingProcessor(fps=100)(beat_act)   # ndarray of seconds

    # ── Downbeat tracking ────────────────────────────────────────────────────
    try:
        db_act = RNNDownBeatProcessor()(audio_file)
        # DBNDownBeatTrackingProcessor returns [[time, beat_position], ...]
        # beat_position == 1 marks the downbeat (bar start).
        db_result = DBNDownBeatTrackingProcessor(
            beats_per_bar=[3, 4], fps=100
        )(db_act)
        downbeats = [float(row[0]) for row in db_result if int(row[1]) == 1]
    except Exception as exc:
        print(f"downbeat model failed ({exc}), falling back to every-4th-beat",
              file=sys.stderr)
        downbeats = beats[::4].tolist()

    # ── BPM from median inter-beat interval ──────────────────────────────────
    if len(beats) > 1:
        bpm = float(60.0 / float(np.median(np.diff(beats))))
    else:
        bpm = 120.0

    return {
        "beats":     [float(b) for b in beats],
        "downbeats": downbeats,
        "bpm":       round(bpm, 2),
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print("usage: beat_tracker.py <audio_file>", file=sys.stderr)
        sys.exit(1)

    try:
        result = track(sys.argv[1])
        print(json.dumps(result))
    except ImportError:
        print("madmom is not installed — run: pip install madmom", file=sys.stderr)
        sys.exit(2)
    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(1)
