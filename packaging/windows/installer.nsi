Unicode True
RequestExecutionLevel user
Name "S3 Studio"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\S3 Studio"
Icon "${APP_ICON}"
UninstallIcon "${APP_ICON}"
SetCompressor /SOLID lzma
VIProductVersion "${APP_VERSION}.0"
VIAddVersionKey "ProductName" "S3 Studio"
VIAddVersionKey "CompanyName" "S3 Studio"
VIAddVersionKey "FileDescription" "S3 Studio Installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "S3 Studio"

!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "MUI2.nsh"

; Keep the install location and uninstall entry used by existing NSIS releases.
!define PRODUCT_KEY "Software\S3 Studio\S3 Studio"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\S3 Studio"
Var Passive

!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipInteractivePage
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE SkipInteractivePage
!define MUI_FINISHPAGE_RUN "$INSTDIR\s3-studio.exe"
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
    SetRegView 64
    SetShellVarContext current
    ${If} $INSTDIR == "$LOCALAPPDATA\S3 Studio"
        ReadRegStr $0 HKCU "${PRODUCT_KEY}" ""
        ${If} $0 != ""
            StrCpy $INSTDIR "$0"
        ${EndIf}
    ${EndIf}
    ${GetOptions} "$CMDLINE" "/P" $0
    ${IfNot} ${Errors}
        StrCpy $Passive 1
    ${EndIf}
FunctionEnd

Function SkipInteractivePage
    ${If} $Passive == 1
        Abort
    ${EndIf}
FunctionEnd

Section "S3 Studio"
    ClearErrors
    SetOutPath "$INSTDIR"
    File /oname=s3-studio.exe "${APP_BINARY}"
    IfErrors failed
    FileOpen $0 "$INSTDIR\s3-studio.install" w
    IfErrors failed
    FileWrite $0 "nsis"
    FileClose $0
    WriteUninstaller "$INSTDIR\uninstall.exe"
    IfErrors failed

    WriteRegStr HKCU "${PRODUCT_KEY}" "" "$INSTDIR"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "S3 Studio"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "S3 Studio"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${APP_VERSION}"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\s3-studio.exe"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$\"$INSTDIR$\""
    WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""
    WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" "$\"$INSTDIR\uninstall.exe$\" /S"
    WriteRegStr HKCU "${UNINSTALL_KEY}" "MainBinaryName" "s3-studio.exe"
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
    CreateShortcut "$SMPROGRAMS\S3 Studio.lnk" "$INSTDIR\s3-studio.exe"
    ${If} $Passive == 1
    ${OrIf} ${Silent}
        CreateShortcut "$DESKTOP\S3 Studio.lnk" "$INSTDIR\s3-studio.exe"
    ${EndIf}
    ${If} $Passive == 1
        SetAutoClose true
    ${EndIf}
    Goto done
    failed:
        SetErrorLevel 1
        Abort
    done:
SectionEnd

Function .onInstSuccess
    ${If} $Passive == 1
    ${OrIf} ${Silent}
        ${GetOptions} "$CMDLINE" "/R" $0
        ${IfNot} ${Errors}
            ExecShell "open" "$INSTDIR\s3-studio.exe"
        ${EndIf}
    ${EndIf}
FunctionEnd

Section "Uninstall"
    SetRegView 64
    SetShellVarContext current
    Delete "$INSTDIR\s3-studio.exe"
    Delete "$INSTDIR\s3-studio.install"
    Delete "$INSTDIR\uninstall.exe"
    Delete "$SMPROGRAMS\S3 Studio.lnk"
    Delete "$DESKTOP\S3 Studio.lnk"
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    DeleteRegValue HKCU "${PRODUCT_KEY}" ""
    DeleteRegKey /ifempty HKCU "${PRODUCT_KEY}"
    RMDir "$INSTDIR"
SectionEnd
