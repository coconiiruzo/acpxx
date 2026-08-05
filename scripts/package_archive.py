#!/usr/bin/env python3
"""Create a byte-reproducible tar.gz from one staged directory."""

import argparse
import gzip
import pathlib
import tarfile


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--epoch", type=int, default=0)
    args = parser.parse_args()
    stage = args.stage.resolve()
    paths = [stage, *sorted(stage.rglob("*"))]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as archive:
                for path in paths:
                    relative = pathlib.Path(stage.name) / path.relative_to(stage)
                    info = archive.gettarinfo(str(path), arcname=str(relative))
                    info.uid = 0
                    info.gid = 0
                    info.uname = "root"
                    info.gname = "wheel"
                    info.mtime = args.epoch
                    if info.isfile():
                        with path.open("rb") as source:
                            archive.addfile(info, source)
                    else:
                        archive.addfile(info)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
