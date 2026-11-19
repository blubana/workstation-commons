# Security policy

## Reporting a vulnerability

Use **Security → Advisories → Report a vulnerability** on the GitHub repository to submit a private vulnerability report. Include the affected revision, deployment configuration, impact, and steps to confirm the issue. Redact credentials and personal data.

## Supported versions

Security fixes target the current default branch. Deployments should update to a revision that includes the relevant fixes.

## Threat model

Compute Manager serves authenticated but mutually untrusted Linux compute users. The daemon identifies clients through Unix socket peer credentials and uses role checks for administrative routes. The privileged helper is a separate boundary for administrative operations. The Windows inference worker accepts authenticated requests over loopback and runs with a limited scheduled-task principal. Users control the models and tensors submitted for inference. Request limits and serial execution reduce resource contention, while the Windows runtime and device drivers remain part of the trusted computing base.
