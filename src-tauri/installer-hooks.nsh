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
