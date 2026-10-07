; Remove only the startup command owned by this installation. Keep user data.
!macro NSIS_HOOK_PREUNINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "CodexPulse"
  StrCmp $0 '$\"$INSTDIR\codexpulse.exe$\" --startup' 0 +2
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "CodexPulse"
!macroend
