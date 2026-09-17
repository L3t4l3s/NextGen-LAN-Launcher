; The launcher starts its own Resilio Sync from the install folder and keeps
; it alive, so stopping the engine alone is pointless: the launcher starts a
; new one, and that one holds "Resilio Sync.exe" when the installer overwrites
; it. The installer's own "close the application?" prompt runs *after* this
; hook, so the launcher is closed here first and the engine second.
; Matched by path, not by name: the engine may be called "Resilio Sync.exe",
; "rslsync.exe" or "btsync.exe", and the path is what says it is ours. A
; Resilio the user installed themselves lives elsewhere and is left alone.
; The string is delimited with backticks so the shell can use double quotes
; and PowerShell single quotes; `\"` is not an escape in NSIS and reached
; PowerShell as a literal backslash, which is why the first version of this
; hook matched nothing. `$$` is a literal dollar, for PowerShell's `$_`.
!macro StopBundledResilio
  ; `uninstall.exe` is left out of both filters: an upgrade runs it from the
  ; install folder itself (`_?=$INSTDIR`, no copy to the temp folder), so it
  ; would be stopping itself.
  DetailPrint "Closing the launcher and its sync engine"
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "Get-Process -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' -and $$_.Path -notlike '$INSTDIR\resilio\*' -and $$_.Path -notlike '*\uninstall.exe' } | Stop-Process -Force -ErrorAction SilentlyContinue"`
  Pop $0
  DetailPrint "Launcher stop returned $0"
  ; It supervises the engine in a loop, so give the last tick time to pass
  ; before the engine itself is stopped.
  Sleep 1000
  ; Everything still running from the install folder, engine included, and
  ; anything that came back while we were stopping: up to ten seconds until
  ; the folder is free. Whatever is left would fail the whole install.
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "for ($$i = 0; $$i -lt 20; $$i++) { $$p = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $$_.Path -like '$INSTDIR\*' -and $$_.Path -notlike '*\uninstall.exe' }); if ($$p.Count -eq 0) { exit 0 }; $$p | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500 }; exit 1"`
  Pop $0
  DetailPrint "Install folder free: $0 (0 = yes)"
  ; Give Windows a moment to release the file handles of everything stopped
  ; above, before the installer starts overwriting them.
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
