# Register the Windows scheduled task that boots the Kamerton WSL distro
# at system startup (naryad Н1-02). With systemd=true in /etc/wsl.conf,
# starting the distro brings up systemd, and the enabled kamerton.service
# and kamerton-backup.timer start on their own; the explicit systemctl
# start is idempotent and also covers a WSL instance that was shut down
# manually while Windows kept running.
# Run in an elevated PowerShell (Administrator). Idempotent: the task is
# replaced on every run (-Force).
#
#   powershell -ExecutionPolicy Bypass -File self-host\install-autostart.ps1
#   powershell -ExecutionPolicy Bypass -File self-host\install-autostart.ps1 -DistroName Ubuntu-22.04

param(
  [string]$DistroName = "Ubuntu"
)

$Action = New-ScheduledTaskAction -Execute "wsl.exe" `
  -Argument "-d $DistroName -u root -- systemctl start kamerton.service kamerton-backup.timer"
$Trigger = New-ScheduledTaskTrigger -AtStartup
$Trigger.Delay = 'PT30S'
$Principal = New-ScheduledTaskPrincipal -UserId "SYSTEM" -RunLevel Highest
$Settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
  -StartWhenAvailable -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask -TaskName "Kamerton WSL boot" `
  -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings -Force

"Done. Verify:"
"  schtasks /query /tn 'Kamerton WSL boot' /v"
"After a forced Windows reboot the task must run and"
"  wsl.exe -d $DistroName -u root -- systemctl is-active kamerton.service"
"must print 'active' with no manual steps."
