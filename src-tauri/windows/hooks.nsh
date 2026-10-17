; Before the uninstaller removes files, let the app clean up what it created itself:
; the login-item registration, settings and logs (spec 1.9, 2 "削除").
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\codex-voice-clipboard.exe" --uninstall --assume-yes'
!macroend
