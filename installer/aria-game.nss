Unicode true
RequestExecutionLevel user
SetCompressor /SOLID lzma
!include MUI2.nsh

!define APPDIR "{{APPDIR}}"
!define OUTFILE "{{OUTFILE}}"
!define VERSION "{{VERSION}}"
!define ICONFILE ""

!define PRODUCT_NAME "{{PRODUCT_NAME}}"
!define PUBLISHER "{{PUBLISHER}}"
!define REGKEY "Software\${PUBLISHER}\${PRODUCT_NAME}"
!define RUN_ARGS "--run-mode release"
!define PLAYER_EXE "{{PLAYER_FILENAME}}"

!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "${PRODUCT_NAME} Setup"
!define MUI_WELCOMEPAGE_TEXT "${PRODUCT_NAME} will be installed on this PC.$\r$\n$\r$\nClick Next to continue."
!define MUI_DIRECTORYPAGE_TEXT_TOP "Choose the install folder. The default is usually fine."
!define MUI_INSTFILESPAGE_COLORS "FFFFFF 202020"
!define MUI_FINISHPAGE_TITLE "Installation Complete"
!define MUI_FINISHPAGE_TEXT "${PRODUCT_NAME} has been installed."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${PLAYER_EXE}"
!define MUI_FINISHPAGE_RUN_PARAMETERS "${RUN_ARGS}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${PRODUCT_NAME}"
!define MUI_FINISHPAGE_LINK "Open install folder"
!define MUI_FINISHPAGE_LINK_LOCATION "$INSTDIR"
!define MUI_UNFINISHPAGE_TITLE "${PRODUCT_NAME} Uninstall"
!define MUI_UNFINISHPAGE_TEXT "Remove ${PRODUCT_NAME} from this PC."

Name "${PRODUCT_NAME} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\${PUBLISHER}\${PRODUCT_NAME}"
InstallDirRegKey HKCU "${REGKEY}" "InstallDir"
!if "${ICONFILE}" != ""
  Icon "${ICONFILE}"
  UninstallIcon "${ICONFILE}"
!endif

VIProductVersion "1.0.0.0"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=1033 "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright ${PUBLISHER}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
!if "${ICONFILE}" != ""
VIAddVersionKey /LANG=1041 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1041 "CompanyName" "${PUBLISHER}"
VIAddVersionKey /LANG=1041 "FileDescription" "${PRODUCT_NAME} Setup"
VIAddVersionKey /LANG=1041 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1041 "LegalCopyright" "Copyright ${PUBLISHER}"
VIAddVersionKey /LANG=1041 "ProductVersion" "${VERSION}"
!endif

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Japanese"

Section "Install"
  DetailPrint "Install destination: $INSTDIR"
  SetOutPath "$INSTDIR"
  DetailPrint "Copying game files..."
  File /r "{{APPDIR}}\*.*"

  WriteRegStr HKCU "${REGKEY}" "InstallDir" "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  DetailPrint "Creating shortcuts..."
  CreateDirectory "$SMPROGRAMS\${PUBLISHER}"
  CreateShortCut "$SMPROGRAMS\${PUBLISHER}\${PRODUCT_NAME}.lnk" "$INSTDIR\${PLAYER_EXE}" "${RUN_ARGS}" "$INSTDIR\${PLAYER_EXE}" 0 SW_SHOWNORMAL "" "${PRODUCT_NAME}" "$INSTDIR"
  CreateShortCut "$SMPROGRAMS\${PUBLISHER}\Uninstall ${PRODUCT_NAME}.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${PLAYER_EXE}" "${RUN_ARGS}" "$INSTDIR\${PLAYER_EXE}" 0 SW_SHOWNORMAL "" "${PRODUCT_NAME}" "$INSTDIR"
SectionEnd

Section "Uninstall"
  Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
  Delete "$SMPROGRAMS\${PUBLISHER}\${PRODUCT_NAME}.lnk"
  Delete "$SMPROGRAMS\${PUBLISHER}\Uninstall ${PRODUCT_NAME}.lnk"
  RMDir "$SMPROGRAMS\${PUBLISHER}"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "${REGKEY}"
SectionEnd
