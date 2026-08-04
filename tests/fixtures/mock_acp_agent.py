#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import time


def send(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", action="store_true")
    parser.add_argument(
        "--mode", choices=("normal", "crash", "malformed", "grandchild"), default="normal"
    )
    parser.add_argument("--delay", type=float, default=0.0)
    parser.add_argument("--pid-file")
    args = parser.parse_args()
    if args.version:
        print("mock-acp 0.1.0")
        return 0

    session_id = "mock-session-1"
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        request_id = request.get("id")
        if method == "initialize":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "protocolVersion": 1,
                        "agentCapabilities": {},
                        "authMethods": [],
                        "agentInfo": {"name": "mock-acp", "version": "0.1.0"},
                    },
                }
            )
        elif method == "session/new":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {"sessionId": session_id},
                }
            )
        elif method == "session/prompt":
            if args.mode == "crash":
                return 17
            if args.mode == "malformed":
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {"stopReason": 123},
                    }
                )
                continue
            if args.mode == "grandchild":
                child = subprocess.Popen(
                    [sys.executable, "-c", "import time; time.sleep(60)"]
                )
                with open(args.pid_file, "w", encoding="utf-8") as pid_file:
                    pid_file.write(str(child.pid))
            print("mock provider log", file=sys.stderr, flush=True)
            if args.delay:
                time.sleep(args.delay)
            send(
                {
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": {"type": "text", "text": "mock-ok"},
                        },
                    },
                }
            )
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {"stopReason": "end_turn"},
                }
            )
        elif request_id is not None:
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": -32601, "message": "method not found"},
                }
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
