# Linux continuation implementation plan

Spec: `docs/design/2026-10-30-layout-and-linux-design.md` (Linux decisions reiterated by the owner).

Work stays uncommitted on the existing feature branch. Preserve the starting dirty tree.
Use `target/agent-build`; run builds and tests sequentially. Never operate the owner's desktop,
clipboard, credentials or provider. Desktop behavior remains unobserved until an owner test.

- [x] Add a session-lived RemoteDesktop/EI keyboard worker, before ydotool fallback. Never retry
  an uncertain injection through another backend. Keep voice-edit selection capture fail-closed
  until a fresh clipboard selection can be proven.
- [x] Implement XGrabKey, XTest and foreground discovery in a Linux-only X11 module; implement
  GTK layer-shell/X11 overlay with runtime availability and notification fallback.
- [x] Keep undelivered Linux results in session memory and expose snapshots to Home. Confirm
  Linux setup shortcuts without recording; add separate Wayland cancel and no-tray guidance.
- [x] Add Linux packaging, signed release aggregation and full Linux app CI, preserving all
  Windows names/signing checks. Enable AppImage updates only if runtime safeguards are verified.
- [x] Integrate runtime capabilities and run every requested gate, sequentially; record exact
  outcomes and remaining acceptance in `linux-status.md`.

Review focus: unavailable/revoked portals; input duplication after uncertain delivery; clipboard
write failure with History off; no tray/layer-shell session; mixed-platform signed asset tampering.
New dependencies must be Linux-only and their purposes recorded. Build/test evidence is separate
from desktop acceptance, package execution and Windows compilation.

Verification outcome: Linux build/clippy/fmt and UI/release checks pass; full Rust suite remains
blocked by sandbox sockets (509 pass, 91 fail, 3 ignored). Voice-edit capture and native acceptance
remain explicitly pending in the continuation report. No commit was made.
