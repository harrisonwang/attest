import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class DemoRecordingTests(unittest.TestCase):
    def test_recording_contains_complete_red_to_green_evidence(self) -> None:
        cast_path = ROOT / "demo/attest-fix/attest-fix.cast"
        video_path = ROOT / "demo/attest-fix/attest-fix.mp4"
        records = [json.loads(line) for line in cast_path.read_text().splitlines()]
        header = records[0]
        events = records[1:]
        timestamps = [event[0] for event in events]
        output = "".join(event[2] for event in events)

        self.assertEqual(header["version"], 2)
        self.assertGreaterEqual(header["duration"], 60)
        self.assertLessEqual(header["duration"], 90)
        self.assertEqual(timestamps, sorted(timestamps))
        self.assertAlmostEqual(timestamps[-1], header["duration"])
        self.assertIn('"token": "src/legacy_auth.rs"', output)
        self.assertIn("-Authentication starts in `src/legacy_auth.rs`.", output)
        self.assertIn("+Authentication starts in `src/auth.rs`.", output)
        self.assertIn("1 verified, 0 broken", output)
        video = video_path.read_bytes()
        self.assertGreater(len(video), 50_000)
        self.assertEqual(video[4:8], b"ftyp")


if __name__ == "__main__":
    unittest.main()
