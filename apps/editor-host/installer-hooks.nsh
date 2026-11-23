!define ENGINE_DASHBOARD_EXE "game-engine-admin-dashboard.exe"
!define ENGINE_DASHBOARD_SHORTCUT "Game Engine Admin Dashboard.lnk"

Var InstallEngineDashboard

!macro NSIS_HOOK_PREINSTALL
  StrCpy $InstallEngineDashboard 0
  ${If} ${FileExists} "$INSTDIR\${ENGINE_DASHBOARD_EXE}"
    StrCpy $InstallEngineDashboard 1
  ${ElseIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    ${IfNot} ${Silent}
      MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Install the optional Admin Dashboard for logs, metrics and the task board?" IDNO +2
      StrCpy $InstallEngineDashboard 1
    ${EndIf}
  ${EndIf}

  ${If} $InstallEngineDashboard = 1
    !insertmacro CheckIfAppIsRunning "${ENGINE_DASHBOARD_EXE}" "Game Engine Admin Dashboard"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $InstallEngineDashboard = 1
    SetOutPath "$INSTDIR"
    ClearErrors
    File "/oname=${ENGINE_DASHBOARD_EXE}" "$%GAME_ENGINE_ADMIN_BUNDLE_EXE%"
    ${If} ${Errors}
      Abort "Could not install the Admin Dashboard."
    ${EndIf}
    ${If} $NoShortcutMode <> 1
      CreateShortCut "$SMPROGRAMS\${ENGINE_DASHBOARD_SHORTCUT}" "$INSTDIR\${ENGINE_DASHBOARD_EXE}"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} ${FileExists} "$INSTDIR\${ENGINE_DASHBOARD_EXE}"
    !insertmacro CheckIfAppIsRunning "${ENGINE_DASHBOARD_EXE}" "Game Engine Admin Dashboard"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro IsShortcutTarget "$SMPROGRAMS\${ENGINE_DASHBOARD_SHORTCUT}" "$INSTDIR\${ENGINE_DASHBOARD_EXE}"
  Pop $0
  ${If} $0 = 1
    !insertmacro UnpinShortcut "$SMPROGRAMS\${ENGINE_DASHBOARD_SHORTCUT}"
    Delete "$SMPROGRAMS\${ENGINE_DASHBOARD_SHORTCUT}"
  ${EndIf}
  Delete "$INSTDIR\${ENGINE_DASHBOARD_EXE}"
  RMDir "$INSTDIR"
!macroend
