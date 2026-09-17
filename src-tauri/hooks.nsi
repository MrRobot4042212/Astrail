; Meteor NSIS installer hooks.
;
; Historically the post-install hook offered to set the `RUNASADMIN` AppCompat
; flag so Meteor would always start elevated. That was removed on purpose: an
; always-elevated launcher passes its elevated token to **every game it spawns**,
; and Meteor installs into a user-writable directory, so anything able to write
; there would gain a silent path to administrator. Admin is opt-in per session
; instead ("Reiniciar como administrador"), and only the metrics sidecars need it.
;
; The hooks below now only clean up that legacy flag.

!macro NSIS_HOOK_POSTINSTALL
  ; Migration for installs made by Meteor <= 0.1.1 that accepted the prompt.
  DeleteRegValue HKCU "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\Meteor.exe"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers" "$INSTDIR\Meteor.exe"
  ; Best effort: remove the elevated logon task older versions could create.
  nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /Delete /TN MeteorAutostart /F'
  Pop $0
  ; "Start with Windows": the Run value and the enable/disable state Task
  ; Manager keeps next to it, so an uninstall leaves no dangling startup entry.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Meteor"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Meteor"
  ; Single-file .NET sidecar builds before 0.1.4 self-extracted native libraries
  ; here; nothing else uses the directory.
  RMDir /r "$TEMP\.net\cputemp"
!macroend
