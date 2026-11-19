import logging
import stat


def safe_mode(path, mode):
    if stat.S_ISREG(mode) and not mode & (stat.S_IWGRP | stat.S_IWOTH | stat.S_IROTH):
        return True
    # nosemgrep: python.lang.security.audit.logging.logger-credential-leak.python-logger-credential-disclosure -- logs the file path only, never its content
    logging.getLogger(__name__).warning("Refusing credential file with unsafe permissions: %s", path)
    return False
