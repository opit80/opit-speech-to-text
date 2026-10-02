; NSIS hooks for the Opit Speech to Text installer
; (tauri.conf.json > bundle > windows > nsis > installerHooks).
;
; Tauri's uninstaller already removes the HKCU Run value named after productName and, when
; "Delete the application data" is ticked, %APPDATA%\<identifier> and %LOCALAPPDATA%\<identifier>
; (WebView2 data). Our own data lives in %APPDATA%\opit-speech-to-text and the API keys in
; Windows Credential Manager, so the same checkbox removes those too.
; Never in update mode: the updater runs the installer with /UPDATE and the data must survive it.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIfNot} ${Silent}
    ; Tauri checks again after this hook, but cancellation must precede any credential deletion.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ; The exe is still in place here. It deletes this app's Credential Manager entries and exits.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-credentials'
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIfNot} ${Silent}
    SetShellVarContext current
    RmDir /r "$APPDATA\opit-speech-to-text"
  ${EndIf}
!macroend
