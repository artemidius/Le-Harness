#!/usr/bin/env python3
"""Exercise the real hidden-input path against a loopback Mistral fixture."""

from __future__ import annotations

import http.server
import json
import os
import pty
import select
import signal
import sys
import termios
import tempfile
import threading
import time


PROMPT = b"Mistral API key (hidden; saved in OS credential store): "
CHAT_PROMPT = b"Select a Mistral chat model for this process:"
FIM_PROMPT = b"Select a Mistral FIM model for this process:"
SENTINEL = "pty-secret-sentinel"


class ModelHandler(http.server.BaseHTTPRequestHandler):
    authorization: str | None = None
    request_path: str | None = None
    request_body: dict[str, object] | None = None
    request_bodies: list[dict[str, object]] = []

    def do_GET(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
        if self.path != "/v1/models":
            self.send_error(404)
            return
        type(self).authorization = self.headers.get("Authorization")
        body = json.dumps(
            {
                "object": "list",
                "data": [
                    {
                        "id": "fixture-chat-a",
                        "object": "model",
                        "capabilities": {
                            "completion_chat": True,
                            "function_calling": True,
                        },
                    },
                    {
                        "id": "fixture-chat-b",
                        "object": "model",
                        "capabilities": {
                            "completion_chat": True,
                            "function_calling": True,
                        },
                    },
                    {
                        "id": "fixture-fim-a",
                        "object": "model",
                        "capabilities": {"completion_fim": True},
                    },
                    {
                        "id": "fixture-fim-b",
                        "object": "model",
                        "capabilities": {"completion_fim": True},
                    },
                ],
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:  # noqa: N802 - required by BaseHTTPRequestHandler
        if self.path != "/v1/chat/completions":
            self.send_error(404)
            return
        type(self).authorization = self.headers.get("Authorization")
        type(self).request_path = self.path
        length = int(self.headers.get("Content-Length", "0"))
        type(self).request_body = json.loads(self.rfile.read(length))
        type(self).request_bodies.append(type(self).request_body)
        if len(type(self).request_bodies) == 1:
            event = json.dumps(
                {
                    "id": "fixture-chat-1",
                    "model": "fixture-chat-b",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {
                                "role": "assistant",
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "fixture-call",
                                        "type": "function",
                                        "function": {
                                            "name": "list_files",
                                            "arguments": "{}",
                                        },
                                    }
                                ],
                            },
                            "finish_reason": "tool_calls",
                        }
                    ],
                }
            )
        else:
            event = json.dumps(
                {
                    "id": "fixture-chat-2",
                    "model": "fixture-chat-b",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {"content": "fixture answer"},
                            "finish_reason": "stop",
                        }
                    ],
                }
            )
        body = f"data: {event}\n\ndata: [DONE]\n\n".encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def read_until(master: int, buffer: bytearray, needle: bytes, deadline: float) -> None:
    while needle not in buffer:
        if time.monotonic() >= deadline:
            raise AssertionError(f"timed out waiting for {needle!r}; output={bytes(buffer)!r}")
        readable, _, _ = select.select([master], [], [], 0.1)
        if readable:
            buffer.extend(os.read(master, 65536))


def collect_until_exit(pid: int, master: int, buffer: bytearray, deadline: float) -> int:
    while time.monotonic() < deadline:
        readable, _, _ = select.select([master], [], [], 0.1)
        if readable:
            try:
                buffer.extend(os.read(master, 65536))
            except OSError:
                pass
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited == pid:
            return os.waitstatus_to_exitcode(status)
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
    raise AssertionError(f"child did not exit; output={bytes(buffer)!r}")


def wait_for_echo(master: int, enabled: bool, deadline: float) -> None:
    while time.monotonic() < deadline:
        is_enabled = bool(termios.tcgetattr(master)[3] & termios.ECHO)
        if is_enabled == enabled:
            return
        time.sleep(0.01)
    state = "enabled" if enabled else "disabled"
    raise AssertionError(f"terminal echo was not {state}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: credential_prompt.py <le-harness-binary>")

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), ModelHandler)
    port = server.server_address[1]
    environment = os.environ.copy()
    for name in [
        "MISTRAL_API_KEY",
        "LE_HARNESS_CHAT_MODEL",
        "LE_HARNESS_FIM_MODEL",
        "LE_HARNESS_UTILITY_MODEL",
        "CI",
    ]:
        environment.pop(name, None)
    environment["LE_HARNESS_MISTRAL_BASE_URL"] = f"http://127.0.0.1:{port}/v1/"
    workspace = tempfile.TemporaryDirectory()
    sessions = os.path.join(workspace.name, "sessions")

    pid, master = pty.fork()
    if pid == 0:
        os.execve(
            sys.argv[1],
            [
                sys.argv[1],
                "--no-credential-store",
                "run",
                "answer from the fixture",
                "--mode",
                "inspect",
                "--root",
                workspace.name,
                "--session-dir",
                sessions,
            ],
            environment,
        )

    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    output = bytearray()
    deadline = time.monotonic() + 15
    try:
        read_until(master, output, PROMPT, deadline)
        wait_for_echo(master, False, deadline)

        os.write(master, SENTINEL.encode() + b"\n")
        while ModelHandler.authorization is None and time.monotonic() < deadline:
            readable, _, _ = select.select([master], [], [], 0.05)
            if readable:
                output.extend(os.read(master, 65536))

        wait_for_echo(master, True, deadline)
        read_until(master, output, CHAT_PROMPT, deadline)
        os.write(master, b"2\n")
        read_until(master, output, FIM_PROMPT, deadline)
        os.write(master, b"1\n")
        status = collect_until_exit(pid, master, output, deadline)
    finally:
        server.shutdown()
        server.server_close()
        os.close(master)
        thread.join(timeout=2)

    rendered = bytes(output)
    assert status == 0, f"fixture-backed run failed with {status}: {rendered!r}"
    assert rendered.count(PROMPT) == 1, rendered
    assert SENTINEL.encode() not in rendered, rendered
    assert b"fixture answer" in rendered, rendered
    assert ModelHandler.authorization == f"Bearer {SENTINEL}"
    assert ModelHandler.request_path == "/v1/chat/completions"
    assert ModelHandler.request_body is not None
    assert ModelHandler.request_body["model"] == "fixture-chat-b"
    assert len(ModelHandler.request_bodies) == 2
    replayed_call = ModelHandler.request_bodies[1]["messages"][-2]["tool_calls"][0]
    assert replayed_call["id"] == "fixture-call"
    assert replayed_call["type"] == "function"
    assert "index" not in replayed_call
    tool_message = ModelHandler.request_bodies[1]["messages"][-1]
    assert tool_message["role"] == "tool"
    assert tool_message["name"] == "list_files"
    assert tool_message["tool_call_id"] == "fixture-call"
    for root, _, files in os.walk(sessions):
        for name in files:
            with open(os.path.join(root, name), "rb") as stored:
                assert SENTINEL.encode() not in stored.read(), name

    cancelled_pid, cancelled_master = pty.fork()
    if cancelled_pid == 0:
        os.execve(
            sys.argv[1],
            [sys.argv[1], "--no-credential-store", "doctor", "--live"],
            environment,
        )

    cancelled_output = bytearray()
    cancelled_deadline = time.monotonic() + 15
    try:
        read_until(cancelled_master, cancelled_output, PROMPT, cancelled_deadline)
        wait_for_echo(cancelled_master, False, cancelled_deadline)
        os.write(cancelled_master, b"\x04")
        wait_for_echo(cancelled_master, True, cancelled_deadline)
        cancelled_status = collect_until_exit(
            cancelled_pid, cancelled_master, cancelled_output, cancelled_deadline
        )
    finally:
        os.close(cancelled_master)

    assert cancelled_status != 0, cancelled_output
    assert b"API key input was cancelled" in cancelled_output, cancelled_output
    workspace.cleanup()


if __name__ == "__main__":
    main()
