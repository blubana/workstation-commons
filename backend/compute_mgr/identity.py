import grp
import os
import pwd
from dataclasses import dataclass


@dataclass(frozen=True)
class Identity:
    user: str
    uid: int
    role: str

    def can_access(self, user: str) -> bool:
        return self.role == "admin" or self.user == user


def resolve_uid(uid: int) -> Identity | None:
    try:
        account = pwd.getpwuid(uid)
    except KeyError:
        return None
    return resolve_account(account)


def resolve_account(account: pwd.struct_passwd) -> Identity:
    gids = set(os.getgrouplist(account.pw_name, account.pw_gid))
    for group, role in (("sudo", "admin"), ("computeadmin", "admin"), ("compute", "user")):
        try:
            if grp.getgrnam(group).gr_gid in gids:
                return Identity(account.pw_name, account.pw_uid, role)
        except KeyError:
            continue
    return Identity(account.pw_name, account.pw_uid, "forbidden")


def compute_accounts() -> list[Identity]:
    try:
        group = grp.getgrnam("compute")
    except KeyError:
        return []
    accounts = {entry.pw_name: entry for entry in pwd.getpwall() if entry.pw_gid == group.gr_gid}
    for name in group.gr_mem:
        try:
            accounts[name] = pwd.getpwnam(name)
        except KeyError:
            continue
    return [resolve_account(accounts[name]) for name in sorted(accounts)]
