#Requires AutoHotkey v2.0
#SingleInstance Force
; Representative AutoHotkey v2 hotkeys that Ddalgak converts (docs/DDALGAK_AHK_MIGRATION.md).

/*
  Remaps and a disabled key
*/
CapsLock::Ctrl
RAlt::RCtrl
Insert::return

; Shortcuts that press other keys or type text
^!j::Send "{Down}"
^+v::Send "^c{Tab}^v"
^!s::SendText "Kind regards,`nKim"
~F14::Send "{Enter 2}"
^F15 up::Send "{End}"

; A website and a program (by a known folder)
^!g::Run "https://github.com/"
^!n::Run "notepad.exe"

; A small macro
^!m::
{
    Send "^a"
    Sleep 200
    SendText "replaced"
    TrayTip "Done", "Ddalgak"
}

; Windows and the clipboard
^!Up::WinMaximize "A"
^!Down::WinMinimize "A"
^!c::A_Clipboard := "copied by a rule"

; Hotstrings
::btw::by the way
:*:;sig::Best regards{Enter}Kim
:C:AHK::AutoHotkey

; Only in one program
#HotIf WinActive("ahk_exe Code.exe")
^!p::Send "^+p"
#HotIf
