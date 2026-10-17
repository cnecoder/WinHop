; WinHop 安装/卸载钩子
;
; 升级安装时旧版若仍在运行，winhop.exe 被占用会导致旧版卸载失败、新文件覆盖不了。
; 安装器是 currentUser 权限，而 WinHop 是提权运行（UAC），taskkill 杀不掉提权进程，
; 因此采用「标记文件 + 应用自退」：写 %APPDATA%\WinHop\quit.flag，
; 应用内看门狗（每 2 秒轮询）检测到后自行 exit(0)，释放 exe 占用。

!macro WINHOP_STOP_RUNNING
  ; 只在本例确实在运行时才处理，避免无谓的安装延迟
  nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq winhop.exe" /NH | findstr /I "winhop.exe"'
  Pop $0 ; 返回码：0 = 进程存在
  Pop $1 ; 输出
  ${If} $0 == "0"
    CreateDirectory "$APPDATA\WinHop"
    FileOpen $2 "$APPDATA\WinHop\quit.flag" w
    FileWrite $2 "quit"
    FileClose $2
    ; 等应用自退：每 500ms 复查一次，最多 6 秒（看门狗 2 秒轮询）
    StrCpy $3 0
    ${Do}
      Sleep 500
      nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq winhop.exe" /NH | findstr /I "winhop.exe"'
      Pop $0
      Pop $1
      ${If} $0 != "0"
        ${Break}
      ${EndIf}
      IntOp $3 $3 + 1
    ${LoopWhile} $3 < 12
    ; 兜底：非提权实例（debug / 关闭提权）可直接结束；提权实例应已由标记文件退出
    nsExec::Exec 'taskkill /F /IM winhop.exe'
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro WINHOP_STOP_RUNNING
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro WINHOP_STOP_RUNNING
!macroend
