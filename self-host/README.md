# Self-hosting Kamerton on the Windows 10 home server (Н1-02)

The server runs 24/7, no rent needed; invariant 7 (data never leaves the
owner's machine) holds physically. WSL2 Ubuntu runs two services under
systemd: `kamerton.service` (mlog serve — routes + the two cron ticks of
Н1-06) and `kamerton-poll.service` (Telegram long polling). The database
lives on `/data`, outside the WSL filesystem.

## One-time setup

1. Data drive mounted at `/data` (see `/etc/fstab` inside WSL); Rust
   toolchain (1.93.1 or newer) available inside WSL.
2. From this repository, inside WSL:

   ```sh
   cargo build --release
   sudo ./deploy/install.sh
   # then edit /data/kamerton/env: KAMERTON_TG_TOKEN and KAMERTON_CHAT_ID
   sudo systemctl restart kamerton.service kamerton-poll.service
   ```

   install.sh puts the pinned mlog v0.28.0 into /usr/local/bin (verifying
   the published SHA256), installs the binary, app.mlog and backup script,
   seeds the arm library, and registers both units.

3. Make sure WSL itself starts on boot (one-time, from Windows as admin):

   ```bat
   schtasks /Create /TN "Kamerton WSL boot" /SC ONSTART /RU SYSTEM ^
     /TR "wsl.exe -d Ubuntu -u root -- true"
   ```

   WSL2 with `systemd=true` (install.sh adds it to `/etc/wsl.conf`) starts
   both services automatically once the instance boots.

## Acceptance checks (naryad Н1-02)

- Forced Windows reboot: both services must come back with no manual
  action:

  ```sh
  systemctl is-active kamerton.service kamerton-poll.service   # -> active
  ```

- The serve banner prints the exec- and env-gate state (mlog's strict
  security model); with the env file from install.sh the banner lists
  `METALOGOS_ALLOW_EXEC=1` and `METALOGOS_SERVE_ALLOW_EXEC=1` as the
  deliberate, loud grants the ticks need.

- The cron ticks are registered and scheduled (the №418 window dedup
  makes restarts safe):

  ```sh
  curl -s http://127.0.0.1:8090/init    # -> scheduled
  curl -s http://127.0.0.1:8090/cron    # -> two jobs, 07:00 and 21:30 +03
  ```

- Weekly backup with restore verification:

  ```sh
  /data/kamerton/backup.sh
  # -> backup: ok -> ... (integrity ok, 6 tables readable)
  ```

## Notes

- Windows 10 is past end of support (Oct 2025); accepted by decision of the
  concept v2, because long polling opens no inbound ports. Revisit through
  a separate ADR if this changes.
- No public address, no webhook, nothing sticks out — the app polls
  Telegram itself; the serve unit listens on 127.0.0.1 only.
