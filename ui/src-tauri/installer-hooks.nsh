; Sanctum — NSIS installer hooks (Tauri 2).
;
; The installer runs per-machine (elevated), so these hooks can register the
; LocalSystem services on install and remove them on uninstall. The three
; service binaries are bundled next to the app via `externalBin`, so they sit
; in $INSTDIR at install time.

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Registering Sanctum protection service and watchdog..."
  nsExec::ExecToLog '"$INSTDIR\sanctum-service.exe" install'
  Pop $0
  DetailPrint "sanctum-service install exited with $0"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Ask the service to tear itself down. It refuses (non-zero) while a lock is
  ; active — in which case we honestly block the uninstall and point at the
  ; Safe-Mode escape, exactly like the in-app copy promises.
  DetailPrint "Removing Sanctum protection..."
  nsExec::ExecToLog '"$INSTDIR\sanctum-service.exe" uninstall'
  Pop $0
  StrCmp $0 "0" sanctum_uninstall_ok
    MessageBox MB_OK|MB_ICONSTOP "A locked Sanctum session is still active, so Sanctum can't be uninstalled yet.$\n$\nWait for the timer to end, or reboot Windows into Safe Mode and run sanctum-recover.exe to remove it. That friction is the point."
    Abort
  sanctum_uninstall_ok:
!macroend
