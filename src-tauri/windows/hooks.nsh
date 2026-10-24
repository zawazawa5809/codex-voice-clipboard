; Before the uninstaller removes files, let the app clean up what it created itself:
; the login-item registration, settings and logs (spec 1.9, 2 "削除").
!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\codex-voice-clipboard.exe" --uninstall --assume-yes'
!macroend

; WebView2 keeps its profile in %LOCALAPPDATA%\<identifier>. Tauri removes it only when
; the user ticks "delete app data", which is off by default, so remove it here.
; An update keeps it.
!macro NSIS_HOOK_POSTUNINSTALL
  ; Without an identifier the line below would remove all of %LOCALAPPDATA%.
  !ifndef BUNDLEID
    !error "BUNDLEID is not defined"
  !endif
  !if "${BUNDLEID}" == ""
    !error "BUNDLEID is empty"
  !endif
  ${If} $UpdateMode <> 1
    RMDir /r "$LOCALAPPDATA\${BUNDLEID}"
  ${EndIf}
!macroend
