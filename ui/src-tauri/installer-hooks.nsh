; Sanctum — NSIS installer hooks (Tauri 2).
;
; The installer runs per-machine (elevated), so these hooks can register the
; LocalSystem services on install and remove them on uninstall. The three
; service binaries are bundled next to the app via `externalBin`, so they sit
; in $INSTDIR at install time.

!macro NSIS_HOOK_PREINSTALL
  ; A previous install may still be running. A service binary is locked while
  ; its process lives, so it can't be overwritten — which would silently strand
  ; a stale service (new app, old service). Stop both services and WAIT until
  ; they are genuinely stopped (or gone) before file extraction.
  ;
  ; Each iteration re-issues both stops: the two services supervise each other
  ; (watchdog restarts the service; the service's reconcile restarts the
  ; watchdog), so a single stop can be undone — re-issuing until both are down
  ; together closes that race. A clean SCM stop does not trip the failure-restart
  ; actions, so once neither is running they stay down.
  ;
  ; If they will not stop within the budget — e.g. a locked session legitimately
  ; refuses STOP — abort honestly instead of laying new files over a locked,
  ; still-running binary (that would reproduce the stale-binary bug and, worse,
  ; a forced kill would skip the service's DNS-restore teardown).
  DetailPrint "Stopping any running Sanctum services before install..."
  StrCpy $1 0
  sanctum_wait_both:
    nsExec::Exec 'sc stop SanctumWatchdog'
    Pop $0
    nsExec::Exec 'sc stop SanctumService'
    Pop $0
    nsExec::Exec 'cmd /c "$SYSDIR\sc.exe" query SanctumWatchdog | "$SYSDIR\findstr.exe" /C:"RUNNING" /C:"PENDING" >nul'
    Pop $2
    StrCmp $2 "0" sanctum_both_busy 0        ; watchdog still active -> keep waiting
    nsExec::Exec 'cmd /c "$SYSDIR\sc.exe" query SanctumService | "$SYSDIR\findstr.exe" /C:"RUNNING" /C:"PENDING" >nul'
    Pop $2
    StrCmp $2 "0" sanctum_both_busy sanctum_both_done  ; service active -> wait, else done
  sanctum_both_busy:
    IntOp $1 $1 + 1
    IntCmp $1 60 sanctum_pre_stuck           ; 60 * 500ms ~= 30s budget
    Sleep 500
    Goto sanctum_wait_both
  sanctum_pre_stuck:
    MessageBox MB_OK|MB_ICONSTOP "Sanctum couldn't stop its background protection to update it.$\n$\nThis usually means a locked session is still active. Wait for the timer to finish, or reboot Windows into Safe Mode and run sanctum-recover.exe, then run the installer again."
    SetErrors
    Abort
  sanctum_both_done:
    ; STATE: STOPPED is reported just before the process image lock is released.
    Sleep 1000
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Registering Sanctum protection service and watchdog..."
  nsExec::ExecToLog '"$INSTDIR\sanctum-service.exe" install'
  Pop $0
  DetailPrint "sanctum-service install exited with $0"
  ; Do not let a failed registration masquerade as a successful install.
  StrCmp $0 "0" sanctum_post_ok
    MessageBox MB_OK|MB_ICONEXCLAMATION "Sanctum's protection service could not be registered (code $0), so filtering is NOT active yet.$\n$\nRun the installer again. If it keeps failing, reboot and retry, or remove any leftover 'SanctumService' entry from Windows Services first."
    SetErrors
  sanctum_post_ok:
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Ask the service to tear itself down. It refuses (non-zero) only while a lock
  ; is active — in which case we honestly block the uninstall and point at the
  ; Safe-Mode escape, exactly like the in-app copy promises.
  DetailPrint "Removing Sanctum protection..."
  nsExec::ExecToLog '"$INSTDIR\sanctum-service.exe" uninstall'
  Pop $0
  StrCmp $0 "0" sanctum_uninstall_ok
    MessageBox MB_OK|MB_ICONSTOP "A locked Sanctum session is still active, so Sanctum can't be uninstalled yet.$\n$\nWait for the timer to end, or reboot Windows into Safe Mode and run sanctum-recover.exe to remove it. That friction is the point."
    Abort
  sanctum_uninstall_ok:
!macroend
