#!/usr/bin/env python3
import argparse
import json
import os
import re
import subprocess
import sys
import time


def send(payload):
    sys.stdout.write(json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--no-auto-update", action="store_true")
    parser.add_argument("--version", action="store_true")
    parser.add_argument("command", nargs="*")
    args = parser.parse_args()
    executable_name = os.path.basename(sys.argv[0])
    if args.version or "version" in args.command:
        if "cursor" in executable_name:
            print("2026.07.20-8cc9c0b")
        elif "codex" in executable_name:
            print("@agentclientprotocol/codex-acp 1.1.9")
        elif "claude" in executable_name:
            print("0.64.2")
        else:
            print("grok 0.2.118")
        return 0

    session_id = "mock-session-1"
    session_cwd = None
    terminal_capability = False
    prompt_count = 0
    for line in sys.stdin:
        request = json.loads(line)
        method = request.get("method")
        request_id = request.get("id")
        if method == "initialize":
            terminal_capability = request.get("params", {}).get(
                "clientCapabilities", {}
            ).get("terminal", False)
            auth_methods = (
                []
                if "no_auth" in executable_name
                else [
                    {
                        "id": (
                            "chat-gpt"
                            if "codex" in executable_name
                            else "cursor_login"
                            if "cursor" in executable_name
                            else "cached_token"
                        ),
                        "name": "Cached test authentication",
                    }
                ]
            )
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {
                        "protocolVersion": 1,
                        "agentCapabilities": {},
                        "authMethods": auth_methods,
                        "agentInfo": {"name": "mock-acp", "version": "0.1.0"},
                    },
                }
            )
        elif method == "authenticate":
            if "auth_hang" in executable_name:
                time.sleep(60)
            send({"jsonrpc": "2.0", "id": request_id, "result": {}})
        elif method == "session/new":
            session_cwd = request.get("params", {}).get("cwd")
            send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": {"sessionId": session_id},
                }
            )
        elif method == "session/prompt":
            prompt_count += 1
            prompt = json.dumps(request.get("params", {}))
            mode_match = re.search(r"__fake_mode=([a-z_]+)", prompt)
            mode = mode_match.group(1) if mode_match else "normal"
            delay_match = re.search(r"__fake_delay=([0-9.]+)", prompt)
            delay = float(delay_match.group(1)) if delay_match else 0.0
            pid_file_match = re.search(r"__fake_pid_file=([^\s\"}]+)", prompt)
            pid_file_path = pid_file_match.group(1) if pid_file_match else None
            if mode == "crash":
                return 17
            if mode == "transport_close":
                return 0
            if mode == "prompt_failure":
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "error": {"code": -32000, "message": "scripted prompt failure"},
                    }
                )
                continue
            if mode in ("cancel_success", "cancel_ignore"):
                for cancellation_line in sys.stdin:
                    cancellation = json.loads(cancellation_line)
                    if cancellation.get("method") == "session/cancel":
                        if mode == "cancel_success":
                            send(
                                {
                                    "jsonrpc": "2.0",
                                    "id": request_id,
                                    "result": {"stopReason": "cancelled"},
                                }
                            )
                            break
                        continue
                continue
            if mode == "malformed":
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": request_id,
                        "result": {"stopReason": 123},
                    }
                )
                continue
            if mode == "stderr_flood":
                sys.stderr.write("stderr-flood:" + ("e" * (2 * 1024 * 1024)))
                sys.stderr.flush()
            if mode == "grandchild":
                child = subprocess.Popen(
                    [sys.executable, "-c", "import time; time.sleep(60)"]
                )
                with open(pid_file_path, "w", encoding="utf-8") as pid_file:
                    pid_file.write(str(child.pid))
            print("mock provider log", file=sys.stderr, flush=True)
            if delay:
                time.sleep(delay)
            if mode in ("permission", "permission_allow"):
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9001,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": session_id,
                            "_meta": {
                                "fixture": {
                                    "event": "approval",
                                    "secret": "must-not-cross",
                                }
                            },
                            "toolCall": {
                                "toolCallId": "fake-tool-1",
                                "title": "Fake mutation",
                            },
                            "options": [
                                {
                                    "optionId": "reject-once",
                                    "name": "Reject once",
                                    "kind": "reject_once",
                                },
                                {
                                    "optionId": "allow-once",
                                    "name": "Allow once",
                                    "kind": "allow_once",
                                },
                            ],
                        },
                    }
                )
                permission_response = json.loads(sys.stdin.readline())
                if permission_response.get("id") != 9001:
                    return 19
                outcome = permission_response.get("result", {}).get("outcome", {})
                if mode == "permission" and outcome.get("outcome") != "cancelled":
                    return 25
                if mode == "permission_allow" and (
                    outcome.get("outcome") != "selected"
                    or outcome.get("optionId") != "allow-once"
                ):
                    return 26
            if mode == "permission_disconnect":
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9002,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": session_id,
                            "toolCall": {
                                "toolCallId": "disconnecting-tool",
                                "title": "Disconnect during permission",
                            },
                            "options": [
                                {
                                    "optionId": "allow-once",
                                    "name": "Allow once",
                                    "kind": "allow_once",
                                }
                            ],
                        },
                    }
                )
                return 29
            if mode == "terminal":
                if not terminal_capability:
                    return 20
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9100,
                        "method": "terminal/create",
                        "params": {
                            "sessionId": session_id,
                            "command": "/bin/sh",
                            "args": ["-c", "printf terminal-ok"],
                            "cwd": session_cwd,
                            "outputByteLimit": 1024,
                        },
                    }
                )
                created = json.loads(sys.stdin.readline())
                terminal_id = created.get("result", {}).get("terminalId")
                if not terminal_id:
                    return 21
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9101,
                        "method": "terminal/wait_for_exit",
                        "params": {
                            "sessionId": session_id,
                            "terminalId": terminal_id,
                        },
                    }
                )
                waited = json.loads(sys.stdin.readline())
                if waited.get("result", {}).get("exitCode") != 0:
                    return 22
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9102,
                        "method": "terminal/output",
                        "params": {
                            "sessionId": session_id,
                            "terminalId": terminal_id,
                        },
                    }
                )
                terminal_output = json.loads(sys.stdin.readline())
                if terminal_output.get("result", {}).get("output") != "terminal-ok":
                    return 23
                send(
                    {
                        "jsonrpc": "2.0",
                        "id": 9103,
                        "method": "terminal/release",
                        "params": {
                            "sessionId": session_id,
                            "terminalId": terminal_id,
                        },
                    }
                )
                released = json.loads(sys.stdin.readline())
                if "result" not in released:
                    return 24
            chunks = 9000 if mode == "output_flood" else 1
            content = (
                "x" * 1024
                if mode == "output_flood"
                else "terminal-ok"
                if mode == "terminal"
                else "mock-ok"
            )
            update_session_id = (
                "mock-session-changed"
                if mode == "session_change" and prompt_count > 1
                else session_id
            )
            if mode == "rich_events":
                send(
                    {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": update_session_id,
                            "update": {
                                "sessionUpdate": "agent_thought_chunk",
                                "content": {"type": "text", "text": "thinking"},
                            },
                        },
                    }
                )
                send(
                    {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": update_session_id,
                            "update": {
                                "sessionUpdate": "tool_call",
                                "toolCallId": "fake-tool-rich",
                                "title": "Inspect fixture",
                                "status": "in_progress",
                                "_meta": {
                                    "fixture": {
                                        "nestedAgentId": "subagent-1",
                                        "terminalId": "term-fixture",
                                    }
                                },
                            },
                        },
                    }
                )
                send(
                    {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": update_session_id,
                            "update": {
                                "sessionUpdate": "tool_call_update",
                                "toolCallId": "fake-tool-rich",
                                "status": "completed",
                            },
                        },
                    }
                )
            for _ in range(chunks):
                send(
                    {
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": update_session_id,
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": {"type": "text", "text": content},
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
