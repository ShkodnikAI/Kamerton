# Self-hosting Kamerton on the Windows 10 home server (Н1-02)

The server runs 24/7, no rent needed; invariant 7 (data never leaves the
owner's machine) holds physically. WSL2 Ubuntu runs the service under
systemd; the database lives on `/data`, outside the WSL filesystem.

## One-time setup

1. Data drive mounted at `/data` (see `/etc/fstab` inside WSL).
2. From this repository, inside WSL:

   ```sh
   sudo ./deploy/install.sh
   # then edit /data/kamerton/env: KAMERTON_TG_TOKEN=<token from BotFather>
   sudo systemctl restart kamerton.service
   ```

3. Make sure WSL itself starts on boot (one-time, from Windows as admin):

   ```bat
   schtasks /Create /TN "Kamerton WSL boot" /SC ONSTART /RU SYSTEM ^
     /TR "wsl.exe -d Ubuntu -u root -- true"
   ```

   WSL2 with `systemd=true` (install.sh adds it to `/etc/wsl.conf`) starts
   `kamerton.service` automatically once the instance boots.

## Acceptance checks (naryad Н1-02)

- Forced Windows reboot: the service must come back with no manual action:

  ```sh
  systemctl is-active kamerton.service   # -> active
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
  Telegram itself.
