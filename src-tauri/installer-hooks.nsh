; The launcher starts its own Resilio Sync from the install folder. The
; installer offers to close the launcher, but the engine is a second process
; and keeps "Resilio Sync.exe" open, so overwriting it fails. Only the copy
; under $INSTDIR is stopped; a Resilio the user installed themselves is none
; of our business.
; Matched by path, not by name: the engine may be called "Resilio Sync.exe",
; "rslsync.exe" or "btsync.exe", and the path is what says it is ours.
; `$_` is an NSIS variable name too, so the pipeline uses the comparison form
; of Where-Object, which needs none.
!macro StopBundledResilio
  DetailPrint "Stopping the bundled sync engine"
  nsExec::ExecToLog 'powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-Process -ErrorAction SilentlyContinue | Where-Object Path -like \"$INSTDIR\resilio\*\" | Stop-Process -Force -ErrorAction SilentlyContinue"'
  Pop $0
  ; Give Windows a moment to release the file handles.
  Sleep 1500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro StopBundledResilio
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopBundledResilio
!macroend

; Windows remembers the icon of every path it has ever displayed. An upgrade
; writes a new executable to the same path, so Explorer, the desktop shortcut
; and the taskbar keep showing the icon of the version before it. Telling the
; shell that associations changed makes it re-read the icon from the file.
!macro NSIS_HOOK_POSTINSTALL
  ; SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, NULL, NULL)
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend

; The same the other way round: without it the uninstalled program keeps its
; icon in the shell's cache.
!macro NSIS_HOOK_POSTUNINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend
