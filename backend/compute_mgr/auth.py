from fastapi import HTTPException, Request

PEER_UID = "compute_mgr.peer_uid"


class Auth:
    """Identity comes only from SO_PEERCRED on the Unix socket (set by server.PeerProtocol)."""

    def __init__(self, resolve_uid):
        self.resolve_uid = resolve_uid

    def authorize(self, uid, *, admin=False):
        if uid is None:
            raise HTTPException(401, "Unix socket peer credentials required")
        identity = self.resolve_uid(uid)
        if identity is None or identity.role not in {"admin", "user"} or (admin and identity.role != "admin"):
            raise HTTPException(403, "Access denied")
        return identity

    def dependency(self, *, admin=False):
        def principal(request: Request):
            return self.authorize(request.scope.get(PEER_UID), admin=admin)

        return principal
