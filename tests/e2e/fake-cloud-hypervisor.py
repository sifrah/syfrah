#!/usr/bin/env python3
"""Fake Cloud Hypervisor binary for E2E testing.

Simulates the cloud-hypervisor API socket interface so that the syfrah
compute layer can be exercised end-to-end inside Docker containers
without KVM.

Usage:
    fake-cloud-hypervisor.py --api-socket /path/to/socket
"""

import argparse
import json
import os
import signal
import socket
import sys
import threading

# VM state machine
vm_state = {"created": False, "booted": False, "paused": False}


def build_response(status_code, body=None):
    """Build a minimal HTTP/1.1 response."""
    reason = {200: "OK", 204: "No Content", 404: "Not Found", 405: "Method Not Allowed"}
    status_text = reason.get(status_code, "Unknown")
    if body is not None:
        body_bytes = json.dumps(body).encode()
        return (
            f"HTTP/1.1 {status_code} {status_text}\r\n"
            f"Content-Type: application/json\r\n"
            f"Content-Length: {len(body_bytes)}\r\n"
            f"\r\n"
        ).encode() + body_bytes
    else:
        return (
            f"HTTP/1.1 {status_code} {status_text}\r\n"
            f"Content-Length: 0\r\n"
            f"\r\n"
        ).encode()


def handle_request(method, path):
    """Route a request and return (status_code, body_or_None, should_exit)."""
    if path == "/api/v1/vmm.ping" and method == "GET":
        return 200, {"build_version": "fake-v1.0"}, False

    if path == "/api/v1/vm.create" and method == "PUT":
        vm_state["created"] = True
        return 204, None, False

    if path == "/api/v1/vm.boot" and method == "PUT":
        vm_state["booted"] = True
        vm_state["paused"] = False
        return 204, None, False

    if path == "/api/v1/vm.info" and method == "GET":
        if vm_state["booted"]:
            state = "Paused" if vm_state["paused"] else "Running"
        elif vm_state["created"]:
            state = "Created"
        else:
            state = "NotCreated"
        info = {
            "config": {
                "cpus": {"boot_vcpus": 2, "max_vcpus": 2},
                "memory": {"size": 536870912},
            },
            "state": state,
            "memory_actual_size": 536870912,
        }
        return 200, info, False

    if path == "/api/v1/vm.shutdown" and method == "PUT":
        vm_state["booted"] = False
        vm_state["paused"] = False
        return 204, None, True  # exit after response

    if path == "/api/v1/vm.delete" and method == "PUT":
        vm_state["created"] = False
        vm_state["booted"] = False
        vm_state["paused"] = False
        return 204, None, False

    if path == "/api/v1/vm.reboot" and method == "PUT":
        vm_state["booted"] = True
        vm_state["paused"] = False
        return 204, None, False

    if path == "/api/v1/vm.resize" and method == "PUT":
        return 204, None, False

    if path == "/api/v1/vm.pause" and method == "PUT":
        vm_state["paused"] = True
        return 204, None, False

    if path == "/api/v1/vm.resume" and method == "PUT":
        vm_state["paused"] = False
        return 204, None, False

    if path == "/api/v1/vm.counters" and method == "GET":
        return 200, {"counters": {}}, False

    return 404, {"error": f"Unknown endpoint: {method} {path}"}, False


def handle_client(conn):
    """Handle a single HTTP request on the Unix socket."""
    try:
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = conn.recv(4096)
            if not chunk:
                return False
            data += chunk

        header_end = data.index(b"\r\n\r\n")
        header_text = data[:header_end].decode("utf-8", errors="replace")
        lines = header_text.split("\r\n")
        request_line = lines[0]
        parts = request_line.split(" ")
        if len(parts) < 2:
            conn.sendall(build_response(400))
            return False

        method = parts[0]
        path = parts[1]

        # Read body if Content-Length is present (we don't need it, but drain it)
        content_length = 0
        for line in lines[1:]:
            if line.lower().startswith("content-length:"):
                content_length = int(line.split(":", 1)[1].strip())

        body_so_far = data[header_end + 4:]
        remaining = content_length - len(body_so_far)
        while remaining > 0:
            chunk = conn.recv(min(remaining, 4096))
            if not chunk:
                break
            remaining -= len(chunk)

        status, body, should_exit = handle_request(method, path)
        conn.sendall(build_response(status, body))
        return should_exit
    except Exception as e:
        sys.stderr.write(f"fake-ch: error handling request: {e}\n")
        return False
    finally:
        conn.close()


def main():
    # Write all output immediately.
    import io
    sys.stdout = io.TextIOWrapper(sys.stdout.buffer, write_through=True)
    sys.stderr = io.TextIOWrapper(sys.stderr.buffer, write_through=True)

    parser = argparse.ArgumentParser(description="Fake Cloud Hypervisor")
    parser.add_argument("--api-socket", required=True, help="Path to API Unix socket")
    args = parser.parse_args()

    socket_path = args.api_socket

    # Clean up stale socket
    if os.path.exists(socket_path):
        os.unlink(socket_path)

    # Print PID to stdout
    os.write(1, f"{os.getpid()}\n".encode())
    os.write(1, f"DIAG:socket={socket_path}\n".encode())

    try:
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        os.write(1, b"DIAG:binding\n")
        server.bind(socket_path)
        os.write(1, b"DIAG:listening\n")
        server.listen(5)
        server.settimeout(1.0)  # allow periodic signal checks
        os.write(1, b"DIAG:ready\n")
    except Exception as e:
        os.write(1, f"DIAG:error={e}\n".encode())
        sys.exit(1)

    shutdown_event = threading.Event()

    def handle_signal(signum, frame):
        shutdown_event.set()

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    while not shutdown_event.is_set():
        try:
            conn, _ = server.accept()
        except socket.timeout:
            continue
        except OSError:
            break

        should_exit = handle_client(conn)
        if should_exit:
            break

    server.close()
    if os.path.exists(socket_path):
        os.unlink(socket_path)
    sys.exit(0)


if __name__ == "__main__":
    main()
