# Step 0 facts capture (naryad Н1-02, Шаг 0).
# Run in PowerShell on the Windows host and paste the output into the
# naryad issue (or save it next to this script). Nothing is modified.

$ErrorActionPreference = 'Continue'

"=== Windows version ==="
Get-CimInstance Win32_OperatingSystem |
  Select-Object Caption, Version, BuildNumber, OSArchitecture | Format-List

"=== WSL version ==="
wsl.exe --version
wsl.exe --status
wsl.exe --list --verbose

"=== Available sleep states (powercfg /a) ==="
powercfg /a

"=== Current timeouts (AC, seconds) ==="
"Standby timeout : " + (powercfg /q SCHEME_CURRENT SUB_SLEEP STANDBYIDLE |
  Select-String 'Power Setting Index' | Select-Object -Last 1)
"Hibernate timeout: " + (powercfg /q SCHEME_CURRENT SUB_SLEEP HIBERNATEIDLE |
  Select-String 'Power Setting Index' | Select-Object -Last 1)
"Disk timeout     : " + (powercfg /q SCHEME_CURRENT SUB_DISK DISKIDLE |
  Select-String 'Power Setting Index' | Select-Object -Last 1)

"=== End of step 0 facts ==="
