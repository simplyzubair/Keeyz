; Keeyz installer hooks (Tauri NSIS).
; Before installing, quietly remove the apps Keeyz replaces: its older names (Zkey, Keyshifterr)
; and Snapperr, whose window snapping is now built into Keeyz. Keeyz itself is upgraded in
; place by the normal installer, and its settings and keys are kept.

!macro KEEYZ_REMOVE_OLD KEY ARGS
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${KEY}" "UninstallString"
  ${If} $R0 != ""
    DetailPrint "Removing an older app that Keeyz replaces (${KEY})"
    ExecWait '$R0 ${ARGS}'
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  nsExec::Exec 'taskkill /F /IM Snapperr.exe'
  Pop $R1
  !insertmacro KEEYZ_REMOVE_OLD "{6E2B7C1A-4F3D-4E8B-9A51-2C7D0B8F5A13}_is1" "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART"
  !insertmacro KEEYZ_REMOVE_OLD "Zkey" "/S"
  !insertmacro KEEYZ_REMOVE_OLD "Keyshifterr" "/S"
  Pop $R1
  Pop $R0
!macroend
