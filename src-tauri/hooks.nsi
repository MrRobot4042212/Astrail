; SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
; SPDX-License-Identifier: GPL-3.0-only
; Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

; Astrail NSIS installer hooks.
;
; Historically the post-install hook offered to set the `RUNASADMIN` AppCompat
; flag so the launcher would always start elevated. That was removed on purpose:
; an always-elevated launcher passes its elevated token to **every game it
; spawns**, and the app installs into a user-writable directory, so anything
; able to write there would gain a silent path to administrator. Admin is opt-in
; per session instead ("Reiniciar como administrador"), and only the metrics
; sidecars need it.
;
; Up to 0.1.3 the app was called Meteor. The product name keys the install
; directory, the uninstall entry, the executable and the shortcuts, so an
; install from before the rename is a different program to this installer.
; The hooks below replace it: its own uninstaller runs in update mode, which
; keeps the library, the settings (the bundle identifier did not change) and
; the "Start with Windows" value (the app moves that one on its first start),
; and its shortcuts are replaced by Astrail ones.

!define LEGACY_NAME "Meteor"
!define LEGACY_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${LEGACY_NAME}"
!define LEGACY_MANUKEY "Software\${LEGACY_NAME}"
!define LEGACY_MANUPRODUCTKEY "${LEGACY_MANUKEY}\${LEGACY_NAME}"
!define APPCOMPAT_LAYERS "Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define STARTUP_APPROVED_KEY "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"

; Install directory of the Meteor install this setup removed; empty otherwise.
Var LegacyDir

; A Meteor shortcut in `dir` that opens the removed install becomes an Astrail
; one. The template creates none in update mode, which is how the in-app
; updater runs this setup, so without this the Start menu entry would vanish.
!macro MigrateLegacyShortcut dir
  !insertmacro IsShortcutTarget "${dir}\${LEGACY_NAME}.lnk" "$LegacyDir\${LEGACY_NAME}.exe"
  Pop $0
  ${If} $0 = 1
    !insertmacro UnpinShortcut "${dir}\${LEGACY_NAME}.lnk"
    Delete "${dir}\${LEGACY_NAME}.lnk"
    ${IfNot} ${FileExists} "${dir}\${PRODUCTNAME}.lnk"
      CreateShortcut "${dir}\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "${dir}\${PRODUCTNAME}.lnk"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  StrCpy $LegacyDir ""
  ; Where the Meteor installer recorded its directory, else the uninstall
  ; entry's InstallLocation, which it stored quoted.
  ReadRegStr $R4 HKCU "${LEGACY_MANUPRODUCTKEY}" ""
  ${If} $R4 == ""
    ReadRegStr $R4 HKCU "${LEGACY_UNINSTKEY}" "InstallLocation"
    StrCpy $R5 $R4 1
    ${If} $R5 == '"'
      StrCpy $R4 $R4 -1 1
    ${EndIf}
  ${EndIf}

  ${If} $R4 != ""
  ${AndIf} ${FileExists} "$R4\uninstall.exe"
    ; The same question the template asks when Astrail itself is running; the
    ; old uninstaller then closes Meteor.
    ${If} $PassiveMode <> 1
    ${AndIfNot} ${Silent}
      nsis_tauri_utils::FindProcessCurrentUser "${LEGACY_NAME}.exe"
      Pop $R5
      ${If} $R5 = 0
        nsis_tauri_utils::StrReplace "$(appRunningOkKill)" "{{product_name}}" "${LEGACY_NAME}"
        Pop $R5
        MessageBox MB_OKCANCEL "$R5" IDOK legacy_close_confirmed
        nsis_tauri_utils::StrReplace "$(appRunning)" "{{product_name}}" "${LEGACY_NAME}"
        Pop $R5
        Abort "$R5"
        legacy_close_confirmed:
      ${EndIf}
    ${EndIf}

    ; `_?=` runs the uninstaller in place so ExecWait really waits for it.
    ClearErrors
    ExecWait '"$R4\uninstall.exe" /P /UPDATE _?=$R4' $R5
    ${IfNot} ${Errors}
    ${AndIf} $R5 = 0
    ${AndIfNot} ${FileExists} "$R4\${LEGACY_NAME}.exe"
      StrCpy $LegacyDir $R4
      ; Running in place, the uninstaller cannot delete itself.
      Delete "$R4\uninstall.exe"
      ${If} $R4 != $INSTDIR
        RMDir "$R4"
      ${EndIf}
      ; Update mode keeps the entries a real uninstall would remove.
      DeleteRegKey HKCU "${LEGACY_UNINSTKEY}"
      DeleteRegKey HKCU "${LEGACY_MANUPRODUCTKEY}"
      DeleteRegKey /ifempty HKCU "${LEGACY_MANUKEY}"
      ; Installs made by Meteor <= 0.1.1 that accepted the elevation prompt.
      DeleteRegValue HKCU "${APPCOMPAT_LAYERS}" "$R4\${LEGACY_NAME}.exe"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $LegacyDir != ""
    !insertmacro MigrateLegacyShortcut "$SMPROGRAMS"
    !insertmacro MigrateLegacyShortcut "$DESKTOP"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Best effort: remove the elevated logon task older versions could create.
  nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /Delete /TN MeteorAutostart /F'
  Pop $0
  ; "Start with Windows": the Run value and the enable/disable state Task
  ; Manager keeps next to it, so an uninstall leaves no dangling startup entry.
  DeleteRegValue HKCU "${RUN_KEY}" "${PRODUCTNAME}"
  DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${PRODUCTNAME}"
  ; The entry from before the rename, if the app never started to move it and
  ; no Meteor install still owns it.
  ReadRegStr $R4 HKCU "${RUN_KEY}" "${LEGACY_NAME}"
  ${If} $R4 != ""
    ; The app stores the executable path quoted and nothing after it.
    StrCpy $R5 $R4 1
    ${If} $R5 == '"'
      StrCpy $R4 $R4 -1 1
    ${EndIf}
    ${IfNot} ${FileExists} "$R4"
      DeleteRegValue HKCU "${RUN_KEY}" "${LEGACY_NAME}"
      DeleteRegValue HKCU "${STARTUP_APPROVED_KEY}" "${LEGACY_NAME}"
    ${EndIf}
  ${EndIf}
  ; Single-file .NET sidecar builds before 0.1.4 self-extracted native libraries
  ; here; nothing else uses the directory.
  RMDir /r "$TEMP\.net\cputemp"
!macroend
