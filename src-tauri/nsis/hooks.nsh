!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "C:\Program Files\USBip\usbip.exe" voxis_usbip_done 0
    DetailPrint "Installing USB/IP virtual audio driver (usbip-win2)..."
    nsExec::ExecToLog 'curl.exe -fsSL --retry 2 -o "$TEMP\voxis-usbip-win2-setup.exe" "https://github.com/vadimgrn/usbip-win2/releases/download/v0.9.8.1/USBip-0.9.8.1-x64.exe"'
    IfFileExists "$TEMP\voxis-usbip-win2-setup.exe" 0 voxis_usbip_done
      nsExec::ExecToLog "powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command $\"Start-Process -FilePath '$TEMP\voxis-usbip-win2-setup.exe' -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/COMPONENTS=main,client' -Verb RunAs -Wait$\""
      Delete "$TEMP\voxis-usbip-win2-setup.exe"
  voxis_usbip_done:
!macroend
