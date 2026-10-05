# Disable sleep and disk idle-off on the Windows host (naryad Н1-02).
# The home server must stay up 24/7 (invariant 7 is physical); a sleeping
# host silently stops long polling and the weekly backup.
# Run in an elevated PowerShell (Administrator). Idempotent: values are
# set to the same numbers on every run.

powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
powercfg /change disk-timeout-ac 0

"Done. The display may still turn off; the system and the disk stay on."
"Verify: powercfg /a must list no standby state as active after this,"
"and self-host/step0-facts.ps1 must report 0 for all three AC timeouts."
