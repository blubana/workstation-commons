import argparse
import errno
import os
import socket
import stat
import struct
from contextlib import ExitStack
from pathlib import Path

import uvicorn
from uvicorn.protocols.http.h11_impl import H11Protocol

from .auth import PEER_UID


def socket_path():
    return Path(os.getenv("CM_SOCK", "/run/compute-mgr/api.sock"))


class PeerProtocol(H11Protocol):
    def connection_made(self, transport):
        super().connection_made(transport)
        connection = transport.get_extra_info("socket")
        if connection is None or connection.family != socket.AF_UNIX:
            return
        try:
            credentials = connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i"))
            _, uid, _ = struct.unpack("3i", credentials)
        except (OSError, struct.error):
            transport.close()
            return
        downstream = self.app

        async def with_peer(scope, receive, send):
            scope[PEER_UID] = uid
            await downstream(scope, receive, send)

        self.app = with_peer


def unlink_same_socket(path, expected):
    try:
        current = path.lstat()
        if (current.st_dev, current.st_ino) == (expected.st_dev, expected.st_ino):
            path.unlink()
    except FileNotFoundError:
        pass


def bind_unix(listener, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        listener.bind(str(path))
    except OSError as exc:
        if exc.errno != errno.EADDRINUSE:
            raise
        existing = path.lstat()
        if not stat.S_ISSOCK(existing.st_mode):
            raise
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as probe:
            probe.settimeout(1)
            try:
                probe.connect(str(path))
            except ConnectionRefusedError:
                unlink_same_socket(path, existing)
            else:
                raise exc
        listener.bind(str(path))


def main():
    argparse.ArgumentParser(description="Serve compute-mgr on its Unix socket").parse_args()
    path = socket_path()
    with ExitStack() as stack:
        unix = stack.enter_context(socket.socket(socket.AF_UNIX, socket.SOCK_STREAM))
        bind_unix(unix, path)
        stack.callback(unlink_same_socket, path, path.lstat())
        path.chmod(0o666)
        unix.listen(2048)
        unix.setblocking(False)
        config = uvicorn.Config("compute_mgr.app:app", http=PeerProtocol, ws="none",
                                proxy_headers=False, access_log=False,
                                lifespan="on", timeout_graceful_shutdown=5)
        server = uvicorn.Server(config)
        server.run(sockets=[unix])
        if not server.started:
            raise SystemExit(1)
