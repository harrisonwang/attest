#!/usr/bin/env python3
"""Capture the real red-to-green fixture and render a launch-ready terminal video."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import tempfile
import textwrap
import time
from pathlib import Path

ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
FONT_CANDIDATES = (
    Path("/System/Library/Fonts/SFNSMono.ttf"),
    Path("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
    Path("/usr/share/fonts/truetype/liberation2/LiberationMono-Regular.ttf"),
)


def capture(binary: Path, run_script: Path, duration: float) -> tuple[dict, list[list]]:
    environment = {
        **os.environ,
        "ATTEST_BINARY": str(binary),
        "DEMO_PAUSE_SECONDS": "0",
        "NO_COLOR": "1",
        "TERM": "xterm-256color",
    }
    process = subprocess.run(
        ["bash", str(run_script)],
        cwd=run_script.parents[2],
        env=environment,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if process.returncode != 0:
        raise RuntimeError(
            f"demo fixture exited {process.returncode}:\n{process.stdout}"
        )
    output = ANSI.sub("", process.stdout).replace("\r\n", "\n")
    required = (
        '"token": "src/legacy_auth.rs"',
        "-Authentication starts in `src/legacy_auth.rs`.",
        "+Authentication starts in `src/auth.rs`.",
        "1 verified, 0 broken",
    )
    missing = [value for value in required if value not in output]
    if missing:
        raise RuntimeError(f"demo output is missing required evidence: {missing}")

    timed_lines: list[tuple[float, str]] = [(0.6, "$ bash demo/attest-fix/run.sh\r\n")]
    elapsed = 1.2
    for line in output.splitlines(keepends=True):
        rendered = line.replace("\n", "\r\n")
        if line.startswith("$ "):
            elapsed += 2.2
        elif line.startswith(("+", "-")) and not line.startswith(("+++", "---")):
            elapsed += 0.55
        elif line.strip() in {"{", "}", "[", "]"}:
            elapsed += 0.15
        else:
            elapsed += 0.09
        timed_lines.append((elapsed, rendered))

    natural_end = timed_lines[-1][0] + 1.5
    scale = duration / natural_end
    events = [[round(timestamp * scale, 3), "o", text] for timestamp, text in timed_lines]
    events.append([duration, "o", "\x1b[0m"])
    header = {
        "version": 2,
        "width": 110,
        "height": 30,
        "timestamp": int(time.time()),
        "duration": duration,
        "title": "attest /attest-fix: broken documentation to verified",
        "env": {"SHELL": "/bin/zsh", "TERM": "xterm-256color"},
    }
    return header, events


def write_cast(path: Path, header: dict, events: list[list]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [json.dumps(header, ensure_ascii=False)]
    lines.extend(json.dumps(event, ensure_ascii=False) for event in events)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def load_font(size: int):
    from PIL import ImageFont

    for path in FONT_CANDIDATES:
        if path.is_file():
            return ImageFont.truetype(str(path), size=size)
    return ImageFont.load_default(size=size)


def visible_lines(output: str, width: int, height: int) -> list[str]:
    logical = ANSI.sub("", output).replace("\r", "").split("\n")
    wrapped = []
    for line in logical:
        wrapped.extend(
            textwrap.wrap(
                line,
                width=width,
                replace_whitespace=False,
                drop_whitespace=False,
                break_long_words=True,
                break_on_hyphens=False,
            )
            or [""]
        )
    return wrapped[-height:]


def line_color(line: str) -> tuple[int, int, int]:
    if line.startswith("$ "):
        return (88, 166, 255)
    if line.startswith("+") and not line.startswith("+++"):
        return (86, 211, 100)
    if line.startswith("-") and not line.startswith("---"):
        return (255, 123, 114)
    if "1 verified, 0 broken" in line:
        return (86, 211, 100)
    if '"broken": 1' in line or '"verdict": "broken"' in line:
        return (255, 166, 87)
    return (201, 209, 217)


def render_frame(path: Path, output: str, elapsed: float, duration: float) -> None:
    from PIL import Image, ImageDraw

    image = Image.new("RGB", (1280, 720), (13, 17, 23))
    draw = ImageDraw.Draw(image)
    font = load_font(21)
    small = load_font(16)
    draw.rounded_rectangle((18, 16, 1262, 704), radius=14, fill=(22, 27, 34))
    draw.rounded_rectangle((18, 16, 1262, 58), radius=14, fill=(35, 42, 52))
    draw.rectangle((18, 42, 1262, 58), fill=(35, 42, 52))
    for index, color in enumerate(((255, 95, 87), (254, 188, 46), (40, 200, 64))):
        x = 42 + index * 24
        draw.ellipse((x, 30, x + 12, 42), fill=color)
    draw.text(
        (110, 27),
        "attest · /attest-fix red → green",
        font=small,
        fill=(201, 209, 217),
    )
    draw.text(
        (1120, 27),
        "65s demo",
        font=small,
        fill=(139, 148, 158),
    )
    lines = visible_lines(output, width=92, height=24)
    for index, line in enumerate(lines):
        draw.text((42, 76 + index * 25), line, font=font, fill=line_color(line))
    progress = min(max(elapsed / duration, 0), 1)
    draw.rectangle((42, 680, 1238, 684), fill=(48, 54, 61))
    draw.rectangle((42, 680, 42 + int(1196 * progress), 684), fill=(88, 166, 255))
    image.save(path)


def render_video(cast_path: Path, video_path: Path, ffmpeg: str) -> None:
    records = [json.loads(line) for line in cast_path.read_text().splitlines()]
    header = records[0]
    events = records[1:]
    duration = float(header["duration"])
    states: list[tuple[float, str]] = [(0.0, "")]
    output = ""
    for timestamp, stream, data in events:
        if stream != "o":
            continue
        output += data
        states.append((float(timestamp), output))

    video_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="attest-demo-frames.") as directory:
        frame_dir = Path(directory)
        concat_lines = []
        for index, (timestamp, state) in enumerate(states[:-1]):
            frame = frame_dir / f"frame-{index:04}.png"
            render_frame(frame, state, timestamp, duration)
            next_timestamp = states[index + 1][0]
            concat_lines.extend(
                [
                    f"file '{frame}'",
                    f"duration {max(next_timestamp - timestamp, 0.01):.3f}",
                ]
            )
        final_frame = frame_dir / f"frame-{len(states) - 1:04}.png"
        render_frame(final_frame, states[-1][1], duration, duration)
        concat_lines.append(f"file '{final_frame}'")
        concat = frame_dir / "frames.txt"
        concat.write_text("\n".join(concat_lines) + "\n", encoding="utf-8")
        subprocess.run(
            [
                ffmpeg,
                "-loglevel",
                "error",
                "-y",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                str(concat),
                "-vf",
                "fps=30,format=yuv420p",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "20",
                "-movflags",
                "+faststart",
                "-t",
                str(duration),
                str(video_path),
            ],
            check=True,
        )


def parse_args() -> argparse.Namespace:
    script_dir = Path(__file__).resolve().parent
    project_root = script_dir.parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", type=Path, default=project_root / "target/release/attest"
    )
    parser.add_argument("--duration", type=float, default=65.0)
    parser.add_argument("--cast", type=Path, default=script_dir / "attest-fix.cast")
    parser.add_argument("--video", type=Path, default=script_dir / "attest-fix.mp4")
    parser.add_argument("--no-video", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        raise SystemExit(f"attest binary not found: {binary}")
    if not 60 <= args.duration <= 90:
        raise SystemExit("--duration must remain within the documented 60–90 second window")
    script_dir = Path(__file__).resolve().parent
    header, events = capture(binary, script_dir / "run.sh", args.duration)
    write_cast(args.cast, header, events)
    if not args.no_video:
        ffmpeg = shutil.which("ffmpeg")
        if ffmpeg is None:
            raise SystemExit("ffmpeg is required unless --no-video is supplied")
        render_video(args.cast, args.video, ffmpeg)
    print(f"wrote {args.cast}")
    if not args.no_video:
        print(f"wrote {args.video}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
