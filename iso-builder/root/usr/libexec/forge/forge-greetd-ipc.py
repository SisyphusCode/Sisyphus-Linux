#!/usr/bin/env python3
"""Minimal greetd IPC shim for Forge-owned COSMIC greeter sessions."""
from __future__ import annotations

import json
import os
import signal
import socket
import struct
import subprocess
import sys
import threading
from collections.abc import Iterable


LOG_PATH = os.environ.get(
    "FORGE_GREETD_IPC_LOG", "/var/lib/forge/cosmic-greeter-session.log"
)
MAX_MESSAGE = 1024 * 1024
running = True
server_socket: socket.socket | None = None


def log(message: str) -> None:
    try:
        os.makedirs(os.path.dirname(LOG_PATH), exist_ok=True)
        with open(LOG_PATH, "a", encoding="utf-8") as handle:
            handle.write(f"forge-greetd-ipc: {message}\n")
    except OSError:
        pass


def read_exact(conn: socket.socket, size: int) -> bytes | None:
    chunks: list[bytes] = []
    remaining = size
    while remaining > 0:
        chunk = conn.recv(remaining)
        if not chunk:
            return None
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def read_message(conn: socket.socket) -> dict[str, object] | None:
    header = read_exact(conn, 4)
    if header is None:
        return None
    (length,) = struct.unpack("@I", header)
    if length > MAX_MESSAGE:
        raise ValueError(f"oversized greetd IPC payload: {length}")
    payload = read_exact(conn, length)
    if payload is None:
        return None
    decoded = json.loads(payload.decode("utf-8"))
    if not isinstance(decoded, dict):
        raise ValueError("greetd IPC payload is not an object")
    return decoded


def write_message(conn: socket.socket, payload: dict[str, object]) -> None:
    encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
    conn.sendall(struct.pack("@I", len(encoded)) + encoded)


def success() -> dict[str, object]:
    return {"type": "success"}


def auth_message(message: str = "Password:") -> dict[str, object]:
    return {
        "type": "auth_message",
        "auth_message_type": "secret",
        "auth_message": message,
    }


def error(description: str) -> dict[str, object]:
    return {
        "type": "error",
        "error_type": "error",
        "description": description,
    }


def env_from_entries(entries: object) -> dict[str, str]:
    env = os.environ.copy()
    if not isinstance(entries, list):
        return env
    for entry in entries:
        if not isinstance(entry, str) or "=" not in entry:
            continue
        key, value = entry.split("=", 1)
        if key:
            env[key] = value
    return env


def command_from_request(value: object) -> list[str]:
    if not isinstance(value, list):
        return []
    return [item for item in value if isinstance(item, str)]


def maybe_start_session(cmd: Iterable[str], env: dict[str, str]) -> bool:
    argv = list(cmd)
    if not argv:
        log("start_session requested without a command")
        return False
    if os.environ.get("FORGE_GREETD_START_SESSIONS") != "1":
        log(f"start_session refused without FORGE_GREETD_START_SESSIONS command={argv!r}")
        return False
    try:
        with open(LOG_PATH, "a", encoding="utf-8") as log_handle:
            child = subprocess.Popen(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=log_handle,
                stderr=subprocess.STDOUT,
                env=env,
                start_new_session=True,
                close_fds=True,
            )
        log(f"spawned session pid={child.pid} command={argv!r}")
        return True
    except OSError as exc:
        log(f"failed to spawn session command={argv!r}: {exc!r}")
        return False


def handle_client(conn: socket.socket) -> None:
    session_user = ""
    with conn:
        while running:
            request = read_message(conn)
            if request is None:
                return
            request_type = request.get("type")
            log(f"request type={request_type!r}")
            if request_type == "create_session":
                username = request.get("username", "")
                session_user = username if isinstance(username, str) else ""
                write_message(conn, auth_message())
            elif request_type == "post_auth_message_response":
                response = request.get("response")
                if isinstance(response, str) and response:
                    write_message(conn, success())
                else:
                    write_message(conn, error("authentication required"))
            elif request_type == "start_session":
                cmd = command_from_request(request.get("cmd"))
                env = env_from_entries(request.get("env"))
                if session_user:
                    env.setdefault("USER", session_user)
                    env.setdefault("LOGNAME", session_user)
                if maybe_start_session(cmd, env):
                    write_message(conn, success())
                else:
                    write_message(conn, error("session start is unavailable in Forge IPC fallback"))
            elif request_type == "cancel_session":
                session_user = ""
                write_message(conn, success())
            else:
                write_message(conn, error(f"unsupported request type: {request_type!r}"))


def shutdown(_signum: int, _frame: object) -> None:
    global running
    running = False
    if server_socket is not None:
        try:
            server_socket.close()
        except OSError:
            pass


def main() -> int:
    global server_socket
    if len(sys.argv) != 2:
        print("usage: forge-greetd-ipc.py SOCKET", file=sys.stderr)
        return 2
    path = sys.argv[1]
    os.makedirs(os.path.dirname(path), exist_ok=True)
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass

    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)

    server_socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server_socket.bind(path)
    os.chmod(path, 0o600)
    server_socket.listen(8)
    log(f"listening on {path}")

    while running:
        try:
            conn, _addr = server_socket.accept()
        except OSError:
            break
        thread = threading.Thread(target=handle_client, args=(conn,), daemon=True)
        thread.start()
    return 0


if __name__ == "__main__":
    sys.exit(main())
